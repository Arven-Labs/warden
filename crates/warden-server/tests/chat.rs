mod support;

use std::time::Duration;

use support::{spin_up_server, MockProvider};
use warden_server::{ClientMessage, ServerConnection, ServerMessage};
use warden_server_protocol::protocol::HistoryRole;

#[tokio::test]
async fn chat_message_gets_answered_by_the_hosted_orchestrator() {
    let addr = spin_up_server(MockProvider::replying("ahoy")).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key")
        .await
        .unwrap();

    conn.send(&ClientMessage::chat("hello")).await.unwrap();

    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatResponse { content, .. }) => assert_eq!(content, "ahoy"),
        other => panic!("expected ChatResponse, got {other:?}"),
    }
}

/// P157: the model's words reach the client as it writes them, in `ChatEvent`s before the answer that closes the turn; the answer
/// has the whole text and what the events carried is the start of it.
#[tokio::test]
async fn the_models_words_arrive_as_chat_events_before_the_answer() {
    use warden_server_protocol::protocol::ChatEventDto;

    // The long piece makes the words held so far worth sending at once, so at least one event comes whatever the machine's speed.
    let long = "x".repeat(300);
    let addr = spin_up_server(MockProvider::streaming(&["Hel", "lo ", &long, "!"])).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();

    conn.send(&ClientMessage::chat("hello")).await.unwrap();

    let mut live = String::new();
    let mut events = 0;
    let content = loop {
        match conn.recv().await.unwrap() {
            Some(ServerMessage::ChatEvent { event: ChatEventDto::Text { text, source: None }, .. }) => {
                live.push_str(&text);
                events += 1;
            }
            Some(ServerMessage::ChatResponse { content, .. }) => break content,
            other => panic!("expected ChatEvent or ChatResponse, got {other:?}"),
        }
    };
    assert_eq!(content, format!("Hello {long}!"), "the answer has the whole text");
    assert!(events >= 1, "the words were shown before the answer");
    assert!(content.starts_with(&live), "what was shown is the start of the answer, in order: {live:?}");
    assert!(live.len() >= 6 + long.len(), "the long piece, and the ones held before it, went out");
}

/// P157: the Stop button. An ordinary turn ends where it is: the answer is what the model had written, it is saved as the
/// conversation's answer, and a turn stopped before any word leaves only the person's question.
#[tokio::test]
async fn stopping_an_ordinary_turn_keeps_what_was_written_as_the_answer() {
    use warden_server_protocol::protocol::ChatEventDto;

    let long = "x".repeat(300);
    let addr = spin_up_server(MockProvider::streaming_then_hanging(&["Hel", "lo ", &long])).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();

    conn.send(&ClientMessage::Chat { message: "hello".into(), conversation_id: Some("c1".into()), attachments: Vec::new(), agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None }).await.unwrap();
    // The words are on the way: the model is still going (it never ends by itself).
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatEvent { event: ChatEventDto::Text { .. }, .. }) => {}
        other => panic!("expected the first words, got {other:?}"),
    }
    conn.send(&ClientMessage::CancelTurn { conversation_id: "c1".into() }).await.unwrap();

    let content = loop {
        match conn.recv().await.unwrap() {
            Some(ServerMessage::ChatEvent { .. }) => {}
            Some(ServerMessage::ChatResponse { content, .. }) => break content,
            other => panic!("expected the answer that ends the stopped turn, got {other:?}"),
        }
    };
    assert_eq!(content, format!("Hello {long}"), "the answer is what had been written");

    conn.send(&ClientMessage::RequestHistory { request_id: 1, limit: None, conversation_id: Some("c1".into()) }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => {
            let turns: Vec<_> = messages.iter().map(|m| (m.role, m.content.as_str())).collect();
            assert_eq!(turns, vec![(HistoryRole::User, "hello"), (HistoryRole::Assistant, format!("Hello {long}").as_str())]);
        }
        other => panic!("expected History, got {other:?}"),
    }

    // The connection works for the next question after a stop.
    conn.ping(3).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::Pong { nonce: 3 })));
}

#[tokio::test]
async fn a_turn_stopped_before_any_word_leaves_only_the_question() {
    let addr = spin_up_server(MockProvider::streaming_then_hanging(&[])).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();

    conn.send(&ClientMessage::Chat { message: "hello".into(), conversation_id: Some("c1".into()), attachments: Vec::new(), agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None }).await.unwrap();
    // Messages of one connection are handled in order, so the stop finds the turn the line above started.
    conn.send(&ClientMessage::CancelTurn { conversation_id: "c1".into() }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatResponse { content, .. }) => assert_eq!(content, ""),
        other => panic!("expected an empty answer, got {other:?}"),
    }

    conn.send(&ClientMessage::RequestHistory { request_id: 1, limit: None, conversation_id: Some("c1".into()) }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => {
            let turns: Vec<_> = messages.iter().map(|m| (m.role, m.content.as_str())).collect();
            assert_eq!(turns, vec![(HistoryRole::User, "hello")], "no empty answer is saved");
        }
        other => panic!("expected History, got {other:?}"),
    }
}

#[tokio::test]
async fn a_failing_model_call_comes_back_as_a_chat_error_not_a_dropped_connection() {
    let addr = spin_up_server(MockProvider::failing()).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key")
        .await
        .unwrap();

    conn.send(&ClientMessage::chat("hello")).await.unwrap();

    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatError { message, .. }) => assert!(message.contains("mock provider failure"), "message was: {message}"),
        other => panic!("expected ChatError, got {other:?}"),
    }

    // The connection itself must still be usable after an error.
    conn.ping(7).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::Pong { nonce: 7 })));
}

/// P40: a device that reconnects gets back the turns it already had, from the same file `Chat`
/// writes to. Since P84 conversations are a person's: another device of the same owner sees them too.
#[tokio::test]
async fn a_reconnecting_device_can_fetch_its_conversation_history() {
    let addr = spin_up_server(MockProvider::replying("ahoy")).await;
    let url = format!("ws://{addr}");

    let mut first = ServerConnection::connect(&url, "dev-1", "Test Device", "test-key").await.unwrap();
    first.send(&ClientMessage::chat("hello")).await.unwrap();
    assert!(matches!(first.recv().await.unwrap(), Some(ServerMessage::ChatResponse { .. })));
    drop(first);

    let mut again = ServerConnection::connect(&url, "dev-1", "Test Device", "test-key").await.unwrap();
    again.send(&ClientMessage::RequestHistory { request_id: 1, limit: None, conversation_id: None }).await.unwrap();
    match again.recv().await.unwrap() {
        Some(ServerMessage::History { request_id, messages }) => {
            assert_eq!(request_id, 1);
            let turns: Vec<_> = messages.iter().map(|m| (m.role, m.content.as_str())).collect();
            assert_eq!(turns, vec![(HistoryRole::User, "hello"), (HistoryRole::Assistant, "ahoy")]);
        }
        other => panic!("expected History, got {other:?}"),
    }

    let mut other_device = ServerConnection::connect(&url, "dev-2", "Other Device", "test-key").await.unwrap();
    other_device.send(&ClientMessage::RequestHistory { request_id: 2, limit: None, conversation_id: None }).await.unwrap();
    match other_device.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => assert_eq!(messages.len(), 2, "the owner's other device sees the same conversation"),
        other => panic!("expected History, got {other:?}"),
    }
}

/// P78: several conversations per device — each `Chat` lands in the one it names, the answer says
/// which, and they can be listed, renamed and deleted.
#[tokio::test]
async fn a_device_keeps_several_conversations_apart() {
    let addr = spin_up_server(MockProvider::replying("ahoy")).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();

    for (id, text) in [("trip", "plan a trip"), ("work", "draft an email")] {
        conn.send(&ClientMessage::Chat { message: text.to_string(), conversation_id: Some(id.to_string()), attachments: Vec::new(), agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None })
            .await
            .unwrap();
        match conn.recv().await.unwrap() {
            Some(ServerMessage::ChatResponse { conversation_id, .. }) => assert_eq!(conversation_id.as_deref(), Some(id)),
            other => panic!("expected ChatResponse, got {other:?}"),
        }
    }

    conn.send(&ClientMessage::RequestHistory { request_id: 1, limit: None, conversation_id: Some("trip".into()) }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => assert_eq!(messages[0].content, "plan a trip"),
        other => panic!("expected History, got {other:?}"),
    }

    conn.send(&ClientMessage::RenameConversation { request_id: 2, conversation_id: "trip".into(), title: "Lisbon".into() }).await.unwrap();
    assert_eq!(conn.recv().await.unwrap(), Some(ServerMessage::ConversationOk { request_id: 2 }));
    conn.send(&ClientMessage::DeleteConversation { request_id: 3, conversation_id: "work".into() }).await.unwrap();
    assert_eq!(conn.recv().await.unwrap(), Some(ServerMessage::ConversationOk { request_id: 3 }));

    conn.send(&ClientMessage::ListConversations { request_id: 4 }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ConversationList { request_id: 4, conversations }) => {
            let listed: Vec<_> = conversations.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect();
            assert_eq!(listed, vec![("trip", "Lisbon")]);
        }
        other => panic!("expected ConversationList, got {other:?}"),
    }

    // Without an id, a turn goes to the default conversation — what a pre-P78 client gets.
    conn.send(&ClientMessage::chat("hello")).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatResponse { conversation_id, .. }) => assert_eq!(conversation_id.as_deref(), Some("default")),
        other => panic!("expected ChatResponse, got {other:?}"),
    }

    conn.send(&ClientMessage::Chat { message: "hello".to_string(), conversation_id: Some("../escape".into()), attachments: Vec::new(), agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None })
        .await
        .unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatError { message, conversation_id, spend_limit_id }) => {
            assert!(message.contains("invalid conversation id"), "message was: {message}");
            assert_eq!(conversation_id.as_deref(), Some("../escape"));
            assert_eq!(spend_limit_id, None);
        }
        other => panic!("expected ChatError, got {other:?}"),
    }
}

/// P157: "regenerate" and "edit" send the turn again with `replace_from`: the conversation keeps what came before that message, has
/// the new pair instead of it and what followed, and a message that isn't there is a `ChatError` that drops nothing.
#[tokio::test]
async fn a_turn_sent_with_replace_from_takes_the_place_of_that_message_and_what_followed() {
    let addr = spin_up_server(MockProvider::replying("ahoy")).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();
    let chat = |text: &str, replace_from: Option<&str>| ClientMessage::Chat {
        message: text.to_string(),
        conversation_id: Some("c1".into()),
        attachments: Vec::new(),
        agent_id: None,
        project_id: None,
        workdir: None,
        thread_of: None,
        replace_from: replace_from.map(str::to_string),
    };

    for text in ["first", "second", "third"] {
        conn.send(&chat(text, None)).await.unwrap();
        assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::ChatResponse { .. })));
    }
    let history = |request_id: u64| ClientMessage::RequestHistory { request_id, limit: None, conversation_id: Some("c1".into()) };
    conn.send(&history(1)).await.unwrap();
    let before = match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => messages,
        other => panic!("expected History, got {other:?}"),
    };
    assert_eq!(before.len(), 6);

    // A message that isn't there: an error, and the conversation is as it was.
    conn.send(&chat("again", Some("nope"))).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatError { message, .. }) => assert!(message.contains("no message 'nope'"), "{message}"),
        other => panic!("expected ChatError, got {other:?}"),
    }
    conn.send(&history(2)).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => assert_eq!(messages, before),
        other => panic!("expected History, got {other:?}"),
    }

    // Edit the second question: the first pair stays, the second and third are gone, the new pair is last.
    conn.send(&chat("second, reworded", Some(&before[2].id))).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::ChatResponse { .. })));
    conn.send(&history(3)).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => {
            let said: Vec<_> = messages.iter().map(|m| m.content.as_str()).collect();
            assert_eq!(said, vec!["first", "ahoy", "second, reworded", "ahoy"]);
            assert_eq!(messages[..2], before[..2], "what came before is the same messages");
        }
        other => panic!("expected History, got {other:?}"),
    }
}

/// P78: a turn can carry images/PDFs, even with no words — they're saved on the user's turn (so
/// history shows them); anything that isn't an image or a PDF is refused before the model.
#[tokio::test]
async fn attachments_ride_along_with_a_turn_and_other_types_are_refused() {
    let addr = spin_up_server(MockProvider::replying("got it")).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();
    let pdf = warden_core::model::Attachment { mime_type: "application/pdf".into(), data: "JVBERi0=".into() };

    conn.send(&ClientMessage::Chat { message: String::new(), conversation_id: None, attachments: vec![pdf.clone()], agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None }).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::ChatResponse { .. })));

    conn.send(&ClientMessage::RequestHistory { request_id: 1, limit: None, conversation_id: None }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::History { messages, .. }) => {
            assert_eq!(messages[0].role, HistoryRole::User);
            assert_eq!(messages[0].attachments, vec![pdf]);
        }
        other => panic!("expected History, got {other:?}"),
    }
    conn.send(&ClientMessage::ListConversations { request_id: 2 }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ConversationList { conversations, .. }) => assert_eq!(conversations[0].title, "Document"),
        other => panic!("expected ConversationList, got {other:?}"),
    }

    let html = warden_core::model::Attachment { mime_type: "text/html".into(), data: "PGgxPg==".into() };
    conn.send(&ClientMessage::Chat { message: "look".into(), conversation_id: None, attachments: vec![html], agent_id: None, project_id: None, workdir: None, thread_of: None, replace_from: None }).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatError { message, .. }) => assert!(message.contains("text/html"), "message was: {message}"),
        other => panic!("expected ChatError, got {other:?}"),
    }
}

/// P78: without a transcriber (tests never call Whisper), `Transcribe` is refused, not ignored.
#[tokio::test]
async fn transcribe_without_a_transcriber_is_an_error() {
    let addr = spin_up_server(MockProvider::replying("x")).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();
    let audio = warden_core::model::Attachment { mime_type: "audio/webm".into(), data: "GkXf".into() };

    conn.send(&ClientMessage::Transcribe { request_id: 5, audio }).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::TranscriptionError { request_id: 5, .. })));
}

/// Regression test for the reader/writer-task split in `server.rs`: a slow `Chat` call used to
/// block the whole connection's read loop, so a `Ping` sent while it was in flight wouldn't get
/// a `Pong` until the chat call finished — long enough to trip the client's real heartbeat
/// timeout. This proves a `Ping` sent mid-`Chat` is answered well before the slow reply lands.
#[tokio::test]
async fn a_ping_sent_during_a_slow_chat_call_is_answered_immediately() {
    let addr = spin_up_server(MockProvider::replying_after("slow reply", Duration::from_secs(2))).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key")
        .await
        .unwrap();

    conn.send(&ClientMessage::chat("hello")).await.unwrap();
    // Give the server a moment to have actually started the (slow) chat call before pinging.
    tokio::time::sleep(Duration::from_millis(100)).await;
    conn.ping(99).await.unwrap();

    let first = tokio::time::timeout(Duration::from_millis(500), conn.recv()).await.expect("Pong took too long — the reader loop was blocked by the in-flight Chat call").unwrap();
    assert!(matches!(first, Some(ServerMessage::Pong { nonce: 99 })), "expected Pong first, got {first:?}");

    let second = conn.recv().await.unwrap();
    assert!(matches!(second, Some(ServerMessage::ChatResponse { .. })), "expected ChatResponse second, got {second:?}");
}

/// Always answers 503, like the Gemini outage of Session 104.
struct Busy;

#[async_trait::async_trait]
impl warden_core::model::ModelProvider for Busy {
    async fn chat_stream(&self, _messages: Vec<warden_core::model::Message>, _tools: Vec<warden_core::tool::ToolSpec>) -> anyhow::Result<warden_core::model::ChatStream> {
        Err(warden_core::model::ProviderHttpError { provider: "Gemini", status: 503, reason: "503 Service Unavailable".into(), body: "high demand".into() }.into())
    }
}

#[tokio::test]
async fn a_down_provider_hands_the_turn_to_a_reserve_and_the_reply_says_so() {
    use std::sync::Arc;
    use warden_core::model::{FallbackProvider, ModelProvider};

    let dir = std::env::temp_dir().join(format!(
        "warden-server-fallback-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let model = FallbackProvider::new(vec![
        ("gemini".to_string(), Arc::new(Busy) as Arc<dyn ModelProvider>),
        ("anthropic".to_string(), Arc::new(MockProvider::replying("from the reserve")) as Arc<dyn ModelProvider>),
    ]);
    let orchestrator = warden_core::orchestrator::Orchestrator::new(Arc::new(model), Arc::new(warden_core::memory::Vault::new(dir.join("vault"))));
    let server = warden_server::Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_open_key_pairing();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());

    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();
    conn.send(&ClientMessage::chat("hello")).await.unwrap();
    match conn.recv().await.unwrap() {
        Some(ServerMessage::ChatResponse { content, fallbacks, .. }) => {
            assert_eq!(content, "from the reserve");
            assert_eq!(fallbacks.len(), 1);
            assert_eq!((fallbacks[0].from.as_str(), fallbacks[0].to.as_str(), fallbacks[0].reason.as_str()), ("gemini", "anthropic", "503 Service Unavailable"));
        }
        other => panic!("expected ChatResponse, got {other:?}"),
    }
}
