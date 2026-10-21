// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Portuguese (`pt`).
class AppLocalizationsPt extends AppLocalizations {
  AppLocalizationsPt([String locale = 'pt']) : super(locale);

  @override
  String get languageLabel => 'Idioma';

  @override
  String get languageEnglish => 'English';

  @override
  String get languagePortuguese => 'Português (Brasil)';

  @override
  String get languageSystem => 'Seguir o sistema';

  @override
  String get commonCancel => 'Cancelar';

  @override
  String get commonSave => 'Salvar';

  @override
  String get commonEdit => 'Editar';

  @override
  String get commonRemove => 'Remover';

  @override
  String get commonDelete => 'Apagar';

  @override
  String get commonKeep => 'Manter';

  @override
  String get commonClose => 'Fechar';

  @override
  String get commonRefresh => 'Atualizar';

  @override
  String get commonLoading => 'Carregando…';

  @override
  String get commonCopy => 'Copiar';

  @override
  String get commonDone => 'Pronto';

  @override
  String get commonTryAgain => 'Tentar de novo';

  @override
  String get folderTitle => 'Pasta de trabalho';

  @override
  String get folderHint =>
      'A IA lê e escreve nesta pasta, e o shell começa nela (cada comando pede o seu sim). Vale para a conversa inteira e não muda depois.';

  @override
  String get folderThisHub => 'Este hub';

  @override
  String get folderYours => 'Suas pastas';

  @override
  String get folderUp => 'Acima';

  @override
  String get folderEmpty => 'Sem subpastas.';

  @override
  String get folderUse => 'Usar esta pasta';

  @override
  String get connTitle => 'Warden — Conexão com o servidor';

  @override
  String get connDiscoverTooltip => 'Procurar hubs na rede local';

  @override
  String get connScanTooltip => 'Escanear QR para preencher a conexão';

  @override
  String get connAccountTooltip => 'Minha conta';

  @override
  String get connSyncTooltip => 'Sync';

  @override
  String get connSkillsTooltip => 'Skills';

  @override
  String get connHost => 'Endereço do servidor';

  @override
  String get connPort => 'Porta';

  @override
  String get connUseTls => 'Usar TLS (wss://)';

  @override
  String get connUseTlsHint =>
      'Para um hub que serve HTTPS — use o nome que o certificado cobre, ex.: hub.tailXXXX.ts.net';

  @override
  String get connPairingKey => 'Chave de pareamento';

  @override
  String get connUsername => 'Usuário';

  @override
  String get connPassword => 'Senha';

  @override
  String get connPasswordHint => 'Só é preciso na primeira vez neste celular';

  @override
  String get connAuthKey => 'Chave de autenticação';

  @override
  String get connAuthKeyHint => 'Chave de pareamento, ou um código de convite';

  @override
  String get connDeviceName => 'Nome do dispositivo';

  @override
  String get connResume => 'Voltar à conversa';

  @override
  String get connDisconnect => 'Desconectar';

  @override
  String get connConnect => 'Conectar';

  @override
  String get connFillAccount =>
      'Preencha endereço, porta, usuário, senha e nome do dispositivo';

  @override
  String get connFillKey =>
      'Preencha endereço, porta, chave de autenticação e nome do dispositivo';

  @override
  String get connLocked =>
      'Seus dados estão trancados porque o hub reiniciou. Entre uma vez com usuário e senha para destrancá-los.';

  @override
  String get connChoosePassword => 'Escolha a sua própria senha para continuar';

  @override
  String connDiscoveryFailed(String error) {
    return 'A busca falhou: $error';
  }

  @override
  String get connNoHub => 'Nenhum hub respondeu na rede local.';

  @override
  String get connHubsFound => 'Hubs encontrados';

  @override
  String get connStatusDisconnected => 'Desconectado';

  @override
  String connStatusDisconnectedWhy(String reason) {
    return 'Desconectado: $reason';
  }

  @override
  String get connStatusConnecting => 'Conectando…';

  @override
  String connStatusConnected(String server) {
    return 'Conectado a $server';
  }

  @override
  String connStatusError(String message) {
    return 'Erro: $message';
  }

  @override
  String get qrTitle => 'Escanear o QR do hub';

  @override
  String get qrNotRecognized =>
      'QR code não reconhecido — escaneie o QR do Espaço de trabalho do desktop.';

  @override
  String get categoryDeleteData => 'Apagar dados';

  @override
  String get categorySpendMoney => 'Gastar dinheiro';

  @override
  String get categoryCriticalInfra =>
      'Mexer na infraestrutura crítica (shell, SSH, nós)';

  @override
  String get categoryExternalMessage =>
      'Enviar uma mensagem externa (navegador, serviços de fora)';

  @override
  String get categoryPublishCode => 'Publicar código';

  @override
  String get categoryImportantConfig =>
      'Mudar configuração importante (tarefas agendadas)';

  @override
  String get categoryElevatedAgent => 'Criar ou mudar agentes';

  @override
  String get autonomy1 => '1 — só responde, sem ferramentas';

  @override
  String get autonomy2 => '2 — sugere: lê, recusa o resto com uma sugestão';

  @override
  String get autonomy3 => '3 — pede o seu sim antes de qualquer mudança';

  @override
  String get autonomy4 => '4 — age sozinho (os tipos marcados ainda perguntam)';

  @override
  String get autonomy5 => '5 — também gerencia os subordinados sozinho';

  @override
  String sideTo(String name) {
    return 'para $name';
  }

  @override
  String sideFrom(String name) {
    return 'de $name';
  }

  @override
  String get sideScheduledTask => 'tarefa agendada';

  @override
  String get sideWebhook => 'webhook';

  @override
  String get sideButton => 'Recados e execuções';

  @override
  String sideButtonCount(int count) {
    return 'Recados e execuções ($count)';
  }

  @override
  String get commonWait => 'Aguarde…';

  @override
  String get commonConfirm => 'Confirmar';

  @override
  String get agentsTitle => 'Agentes';

  @override
  String get agentsTabOrganization => 'Organização';

  @override
  String get agentsTabTasks => 'Tarefas';

  @override
  String get agentsTabActivity => 'Atividade';

  @override
  String get pairingWrong => 'Chave de pareamento errada.';

  @override
  String get pairingLabel => 'Chave de pareamento do hub';

  @override
  String get pairingHelper => 'Pedida a cada mudança; nunca guardada.';

  @override
  String positionTitle(String agent) {
    return 'Posição de $agent';
  }

  @override
  String get positionRole => 'Cargo';

  @override
  String get positionRoleHint => 'ex.: líder de backend';

  @override
  String get positionReportsTo => 'Reporta a';

  @override
  String positionReportsHelper(String agent) {
    return 'Quem reporta a $agent vai junto com ele.';
  }

  @override
  String get positionNobody => 'Ninguém (topo da árvore)';

  @override
  String powersTitle(String agent) {
    return 'Poderes de $agent';
  }

  @override
  String get powersAutonomy => 'Autonomia';

  @override
  String powersLevel5(String agent) {
    return 'O nível 5 só vale para um agente que pode gerenciar agentes, e $agent não pode: isso se liga nas configurações.';
  }

  @override
  String get powersStartTasks => 'Pode começar trabalho em segundo plano';

  @override
  String get powersCreateWorkers => 'Pode criar agentes temporários';

  @override
  String get powersMessageUser => 'Pode começar uma conversa com você';

  @override
  String get powersChooseModels =>
      'Pode escolher o modelo das tarefas que delega';

  @override
  String get powersAsks =>
      'Pede o seu sim antes de (mesmo quando age sozinho):';

  @override
  String limitTitle(String agent) {
    return 'Modelos de $agent';
  }

  @override
  String get limitHint =>
      'Os modelos que ele pode escolher para as tarefas que delega. Nenhum marcado deixa a escolha aberta; com um, ele dita o modelo.';

  @override
  String get limitNone =>
      'O hub não tem provedores nem políticas para escolher.';

  @override
  String limitPolicy(String id) {
    return '$id (política)';
  }

  @override
  String get limitDefault => 'Padrão';

  @override
  String limitDefaultHelper(String agent) {
    return 'O que uma tarefa recebe quando $agent não escolhe.';
  }

  @override
  String get policyNew => 'Nova política';

  @override
  String policyTitle(String id) {
    return 'Política $id';
  }

  @override
  String get policyName => 'Nome';

  @override
  String get policyAnsweredBy => 'Respondida por';

  @override
  String get policyWhen => 'Quando escolher (uma linha)';

  @override
  String get policyWhenHint => 'ex.: trabalho simples e barato';

  @override
  String get policySave => 'Salvar política';

  @override
  String get newAgentTop => 'Novo agente no topo';

  @override
  String newAgentUnder(String parent) {
    return 'Novo agente sob $parent';
  }

  @override
  String get newAgentName => 'Nome';

  @override
  String get newAgentNameHint => 'ex.: revisor';

  @override
  String get newAgentRole => 'Cargo (opcional)';

  @override
  String get newAgentDoes => 'O que ele faz';

  @override
  String get newAgentHelper =>
      'Começa cuidadoso: ferramentas só de leitura, pergunta antes de qualquer mudança, e não pode delegar nem gerenciar agentes até uma pessoa liberar.';

  @override
  String get newAgentAdd => 'Adicionar agente';

  @override
  String get orgRestarts => 'O hub reinicia com a mudança.';

  @override
  String orgChange(String agent) {
    return 'Mudar $agent';
  }

  @override
  String orgAdd(String agent) {
    return 'Adicionar $agent';
  }

  @override
  String orgSavePolicy(String id) {
    return 'Salvar a política $id';
  }

  @override
  String orgRemovePolicy(String id) {
    return 'Remover a política $id';
  }

  @override
  String get delegationOpen =>
      'Aberto: o agente escolhe qualquer modelo para cada tarefa que delega.';

  @override
  String delegationDictated(String model) {
    return 'Ditado: toda tarefa que ele delega roda em $model.';
  }

  @override
  String delegationLimited(int count, String model) {
    return 'Limitado a $count modelos; $model é o que uma tarefa recebe quando o agente não escolhe.';
  }

  @override
  String get taskPending => 'Pendente';

  @override
  String get taskRunning => 'Em andamento';

  @override
  String get taskWaiting => 'Aguardando um agente';

  @override
  String get taskPaused => 'Pausada';

  @override
  String get taskDone => 'Concluída';

  @override
  String get taskFailed => 'Falhou';

  @override
  String get taskCancelled => 'Parada';

  @override
  String get taskActionPause => 'Pausar';

  @override
  String get taskActionResume => 'Retomar';

  @override
  String get taskActionCancel => 'Parar';

  @override
  String get grantDeleteData => 'Apagar dados';

  @override
  String get grantSpendMoney => 'Gastar dinheiro';

  @override
  String get grantCriticalInfra => 'Infraestrutura crítica';

  @override
  String get grantExternalMessage => 'Mensagem externa';

  @override
  String get grantPublishCode => 'Publicar código';

  @override
  String get grantImportantConfig => 'Configuração importante';

  @override
  String get grantElevatedAgent => 'Agente com permissão elevada';

  @override
  String spendWindow(int hours) {
    return 'nas últimas $hours h';
  }

  @override
  String spendUnpricedOne(int count) {
    return '$count chamada sem preço';
  }

  @override
  String spendUnpricedMany(int count) {
    return '$count chamadas sem preço';
  }

  @override
  String spendUnder(String amount) {
    return 'menos de $amount';
  }

  @override
  String spendWithUnpriced(String money, String window, String unpriced) {
    return '$money $window (e $unpriced)';
  }

  @override
  String get agoNow => 'agora há pouco';

  @override
  String agoMinutes(int count) {
    return 'há $count min';
  }

  @override
  String agoHours(int count) {
    return 'há $count h';
  }

  @override
  String agoDays(int count) {
    return 'há $count d';
  }

  @override
  String tallyDone(int count) {
    return '$count concluídas';
  }

  @override
  String tallyFailed(int count) {
    return '$count falharam';
  }

  @override
  String tallyStopped(int count) {
    return '$count paradas';
  }

  @override
  String tallyRunning(int count) {
    return '$count em andamento';
  }

  @override
  String tallyWaiting(int count) {
    return '$count aguardando um agente';
  }

  @override
  String tallyPaused(int count) {
    return '$count pausadas';
  }

  @override
  String tallyPending(int count) {
    return '$count pendentes';
  }

  @override
  String tallyTokens(String tokens) {
    return '$tokens tokens';
  }

  @override
  String tallyDelegated(int count) {
    return 'delegou $count';
  }

  @override
  String tallyActiveAgo(String ago) {
    return 'ativo $ago';
  }

  @override
  String get activityNoAgent => 'O assistente';

  @override
  String activityCreatedAgent(String actor, String target) {
    return '$actor criou o agente $target';
  }

  @override
  String activityRemovedAgent(String actor, String target) {
    return '$actor removeu o agente $target';
  }

  @override
  String activityUpdatedAgent(String actor, String target) {
    return '$actor alterou o agente $target';
  }

  @override
  String activityScheduledRan(String actor, String target) {
    return '$actor rodou a tarefa agendada $target';
  }

  @override
  String activityScheduledFailed(String target) {
    return 'A tarefa agendada $target falhou';
  }

  @override
  String activityWebhookRan(String actor, String target) {
    return '$actor atendeu o webhook $target';
  }

  @override
  String activityWebhookFailed(String target) {
    return 'O webhook $target falhou';
  }

  @override
  String activityDelegated(String actor, String target) {
    return '$actor delegou uma tarefa a $target';
  }

  @override
  String activityStarted(String actor) {
    return '$actor começou a tarefa';
  }

  @override
  String activityDone(String actor) {
    return '$actor concluiu a tarefa';
  }

  @override
  String activityFailed(String actor) {
    return '$actor não conseguiu concluir a tarefa';
  }

  @override
  String activityCancelled(String actor) {
    return 'A tarefa de $actor foi cancelada';
  }

  @override
  String activityNote(String actor, String target) {
    return '$actor deixou um recado para $target';
  }

  @override
  String activityReply(String actor, String target) {
    return '$actor respondeu a $target';
  }

  @override
  String activityMessagedUser(String actor) {
    return '$actor escreveu para você';
  }

  @override
  String get activityToday => 'Hoje';

  @override
  String get activityYesterday => 'Ontem';

  @override
  String activityDate(int day, String month, int year) {
    String _temp0 = intl.Intl.selectLogic(month, {
      '1': 'janeiro',
      '2': 'fevereiro',
      '3': 'março',
      '4': 'abril',
      '5': 'maio',
      '6': 'junho',
      '7': 'julho',
      '8': 'agosto',
      '9': 'setembro',
      '10': 'outubro',
      '11': 'novembro',
      'other': 'dezembro',
    });
    return '$day de $_temp0 de $year';
  }

  @override
  String orgRemoveTitle(String agent) {
    return 'Remover $agent?';
  }

  @override
  String orgRemoveBody(int count, String goesTo) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Os $count subordinados dele passam a reportar a $goesTo. ',
      one: 'O subordinado dele passa a reportar a $goesTo. ',
      zero: '',
    );
    return '${_temp0}As conversas ficam; não dá para desfazer.';
  }

  @override
  String orgRemove(String agent) {
    return 'Remover $agent';
  }

  @override
  String get orgNobodyTop => 'ninguém (o topo da árvore)';

  @override
  String get badgeDelegates => 'delega';

  @override
  String get badgeManages => 'gerencia agentes';

  @override
  String get badgeNotes => 'deixa recados';

  @override
  String get badgeTasks => 'agenda tarefas';

  @override
  String get badgeNoBackground => 'sem trabalho em segundo plano';

  @override
  String get badgeNoTemp => 'sem agentes temporários';

  @override
  String get badgeNoChat => 'não começa conversa';

  @override
  String get badgeNoModels => 'não escolhe modelos';

  @override
  String badgeAutonomy(int level, String label) {
    return 'autonomia $level: $label';
  }

  @override
  String get autonomyShort1 => 'só responde';

  @override
  String get autonomyShort2 => 'sugere';

  @override
  String get autonomyShort3 => 'pergunta antes';

  @override
  String get autonomyShort4 => 'age sozinho';

  @override
  String get autonomyShort5 => 'gerencia sozinho';

  @override
  String badgeAsksFirst(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'pergunta antes: $count tipos de ação',
      one: 'pergunta antes: 1 tipo de ação',
    );
    return '$_temp0';
  }

  @override
  String orgReports(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count subordinados',
      one: '1 subordinado',
    );
    return '$_temp0';
  }

  @override
  String get menuChat => 'Conversar';

  @override
  String get menuTasks => 'Tarefas';

  @override
  String get menuEditPosition => 'Editar posição';

  @override
  String get menuPowers => 'Poderes';

  @override
  String get menuModels => 'Modelos que ele pode escolher';

  @override
  String get menuAddReport => 'Adicionar um subordinado';

  @override
  String get orgHint =>
      'Quem reporta a quem. Um agente que gerencia ou delega a outros alcança só os que estão abaixo dele. Tudo o que um agente muda ainda espera o seu sim.';

  @override
  String get orgNone => 'Nenhum agente ainda.';

  @override
  String get orgNobodyReports =>
      'Ninguém reporta a ninguém ainda: use \"Editar posição\" num agente para escolher o superior dele.';

  @override
  String get orgAddTop => 'Adicionar um agente no topo';

  @override
  String get policiesTitle => 'Políticas de modelo';

  @override
  String get policiesNone =>
      'Nenhuma: um agente que delega vê só os ids dos modelos.';

  @override
  String get grantsTitle => 'Permissões dadas com \"sempre\"';

  @override
  String grantsHintKept(int ttl) {
    String _temp0 = intl.Intl.pluralLogic(
      ttl,
      locale: localeName,
      other:
          'Guardadas num arquivo, sobrevivem a uma reinicialização e expiram após $ttl dias sem uso (cada uso as renova).',
      one: 'Guardadas num arquivo, sobrevivem a uma reinicialização e expiram após 1 dia sem uso (cada uso as renova).',
    );
    return 'Valem por conversa e por agente. $_temp0 Retirar uma faz o agente perguntar de novo.';
  }

  @override
  String get grantsHintSession =>
      'Valem por conversa e por agente. Terminam quando a conexão fecha. Retirar uma faz o agente perguntar de novo.';

  @override
  String get grantsNone => 'Nenhuma ainda.';

  @override
  String get grantDefaultAgent => 'agente padrão';

  @override
  String grantConversation(String id) {
    return 'conversa $id';
  }

  @override
  String grantUsed(String ago) {
    return 'usada $ago';
  }

  @override
  String get grantTakeBack => 'Retirar';

  @override
  String taskControlTitle(String action, String agent) {
    return '$action a tarefa de $agent';
  }

  @override
  String get taskControlStops => 'As subtarefas dela também param.';

  @override
  String get taskControlOnly =>
      'Só uma tarefa rodando neste hub pode ser controlada.';

  @override
  String get taskAnAgent => 'Um agente';

  @override
  String get taskWorking => 'trabalhando';

  @override
  String taskProgress(int finished, int total, int percent, String parts) {
    return '$finished de $total terminadas ($percent%) · $parts';
  }

  @override
  String taskTok(String tokens) {
    return '$tokens tok';
  }

  @override
  String get taskDetailTask => 'Tarefa';

  @override
  String get taskDetailResult => 'Resultado';

  @override
  String get taskDetailWhy => 'Por que parou';

  @override
  String taskNoResult(String state) {
    return '$state: ainda sem resultado.';
  }

  @override
  String get tasksHint =>
      'Tarefas que os agentes passaram uns aos outros em segundo plano neste hub, onde cada uma está e quanto custou.';

  @override
  String tasksOnly(String agent) {
    return 'Só as tarefas de $agent: as que recebeu e as que delegou.';
  }

  @override
  String get tasksShowAll => 'Mostrar todas';

  @override
  String tasksNoneFor(String agent) {
    return 'Nenhuma tarefa envolve $agent ainda.';
  }

  @override
  String get tasksNone =>
      'Nada ainda. Quando um agente delega uma tarefa em \"segundo plano\", ela aparece aqui.';

  @override
  String get activityHint =>
      'O que aconteceu entre os agentes neste hub: quem delegou, começou e terminou o quê, os recados que deixaram uns para os outros e as mensagens que um agente escreveu primeiro para você.';

  @override
  String get activityAll => 'Todos os agentes';

  @override
  String activityNothingFor(String agent) {
    return 'Nada envolve $agent ainda.';
  }

  @override
  String get activityNothing =>
      'Nada ainda. Quando agentes delegarem tarefas, deixarem recados uns para os outros ou escreverem para você, aparece aqui.';

  @override
  String get chatApprovalTitle => 'Aprovação necessária';

  @override
  String chatApprovalCategory(String category) {
    return 'Categoria: $category (este agente pede aprovação para isso)';
  }

  @override
  String get chatDeny => 'Negar';

  @override
  String get chatApprove => 'Aprovar';

  @override
  String chatSyncSnack(String message) {
    return 'Sync: $message';
  }

  @override
  String get chatEditTitle => 'Editar mensagem';

  @override
  String get chatSend => 'Enviar';

  @override
  String chatEditWarning(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          'Enviar isto apaga as $count mensagens depois dela, e as threads que partem delas.',
      one: 'Enviar isto apaga a mensagem depois dela, e as threads que partem dela.',
    );
    return '$_temp0';
  }

  @override
  String chatChannelFailed(String error) {
    return 'Não foi possível abrir o canal: $error';
  }

  @override
  String get chatFilesTitle => 'Arquivos';

  @override
  String get chatFilesNone => 'Nenhuma pasta configurada ainda.';

  @override
  String chatFilesConfigured(String uri) {
    return 'Pasta configurada:\n$uri';
  }

  @override
  String get chatFilesHint =>
      'Deixa o modelo listar e ler arquivos de texto dentro desta pasta. Vale na próxima vez que você conectar.';

  @override
  String get chatFilesClear => 'Limpar';

  @override
  String get chatFilesChoose => 'Escolher pasta';

  @override
  String get chatNewConversation => 'Nova conversa';

  @override
  String chatAs(String agent) {
    return 'como $agent';
  }

  @override
  String get chatChannelsTooltip => 'Canais dos agentes';

  @override
  String chatChannelsTooltipNew(int count) {
    return 'Canais dos agentes ($count novos)';
  }

  @override
  String get chatAgentsTooltip => 'Agentes';

  @override
  String get chatUsageTooltip => 'Uso';

  @override
  String get chatDevicesTooltip => 'Dispositivos';

  @override
  String get chatConversationsTooltip => 'Conversas';

  @override
  String get chatFilesTooltip => 'Arquivos';

  @override
  String get chatDisconnect => 'Desconectar';

  @override
  String get chatThreadBack => 'Voltar à conversa';

  @override
  String get chatChannelBack => 'Voltar às conversas';

  @override
  String get chatWorkBack => 'Voltar aos recados e execuções';

  @override
  String get chatRegenerate => 'Gerar de novo';

  @override
  String get folderBarNone => 'Pasta de trabalho: nenhuma';

  @override
  String get folderBarTooltip =>
      'Escolha uma pasta do hub ou de um nó para a IA trabalhar';

  @override
  String get folderBarClear => 'Remover a pasta';

  @override
  String get channelsNone => 'Nenhum agente configurado ainda.';

  @override
  String get channelsAnswering => 'respondendo…';

  @override
  String get channelsNew => 'novo';

  @override
  String workNone(String agent) {
    return '$agent ainda não trocou recados com outros agentes nem rodou sozinho.';
  }

  @override
  String get agentMenuTooltip => 'Agente';

  @override
  String get agentMenuNone => 'Sem agente';

  @override
  String agentMenuRemoved(String agent) {
    return '$agent (removido)';
  }

  @override
  String attachmentUnsupported(String mime) {
    return '📎 $mime anexado (ainda não dá para reproduzir aqui)';
  }

  @override
  String attachmentDecodeFailed(String mime) {
    return 'Não foi possível decodificar o anexo $mime';
  }

  @override
  String attachmentPlayFailed(String mime) {
    return 'Não foi possível reproduzir o anexo $mime';
  }

  @override
  String get inputHint => 'Mensagem';

  @override
  String get bucketLast7 => 'Últimos 7 dias';

  @override
  String get bucketLast30 => 'Últimos 30 dias';

  @override
  String get bucketOlder => 'Mais antigas';

  @override
  String get drawerRenameTitle => 'Renomear conversa';

  @override
  String get drawerDeleteTitle => 'Apagar a conversa?';

  @override
  String drawerDeleteBody(String title) {
    return '\"$title\" será apagada do hub. Não dá para desfazer.';
  }

  @override
  String get drawerUntitled => 'Sem título';

  @override
  String get drawerAnswering => 'Respondendo…';

  @override
  String get drawerActions => 'Ações da conversa';

  @override
  String get drawerUnpin => 'Desafixar';

  @override
  String get drawerPin => 'Fixar no topo';

  @override
  String get drawerUnarchive => 'Desarquivar';

  @override
  String get drawerArchive => 'Arquivar';

  @override
  String get drawerRename => 'Renomear';

  @override
  String get drawerNew => 'Nova conversa';

  @override
  String get drawerNone => 'Nenhuma conversa ainda.';

  @override
  String get drawerPinned => 'Fixadas';

  @override
  String drawerArchived(int count) {
    return 'Arquivadas ($count)';
  }

  @override
  String get bannerDisconnected => 'Desconectado';

  @override
  String bannerDisconnectedWhy(String reason) {
    return 'Desconectado: $reason';
  }

  @override
  String get bannerConnecting => 'Conectando…';

  @override
  String bannerError(String message) {
    return 'Erro de conexão: $message';
  }

  @override
  String get liveStop => 'Parar';

  @override
  String get thinking => 'Pensando…';

  @override
  String get threadReply => 'Responder na thread';

  @override
  String threadReplies(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count respostas',
      one: '1 resposta',
    );
    return '$_temp0';
  }
}
