import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/protocol/messages.dart';
import 'package:mobile/screens/skills_screen.dart';
import 'package:mobile/services/server_connection.dart' show ConversationException;
import 'package:mobile/services/skill_draft.dart';
import 'package:mobile/services/skills_repository.dart';

import '../helpers/test_app.dart';

/// In-memory stand-in for the Rust bridge, enforcing the two rules the screen depends on: create
/// refuses a taken name, and an invalid skill surfaces as an exception carrying a message.
class _FakeRepository implements SkillsRepository {
  _FakeRepository(List<SkillDto> initial, {this.gallery = const [], Set<String> outdated = const {}})
      : skills = List.of(initial),
        outdated = Set.of(outdated);

  final List<SkillDto> skills;

  /// Names of untouched gallery skills whose gallery text has moved on: the bridge says `outdated` for
  /// them, and installing clears it.
  final Set<String> outdated;

  /// The ready-made skills; their status is worked out against [skills] the way the bridge does it
  /// (not there / same text / there but different).
  final List<SkillDto> gallery;

  @override
  Future<List<CatalogSkillDto>> listCatalog() async => [
        for (final entry in gallery)
          CatalogSkillDto(
            name: entry.name,
            description: entry.description,
            body: entry.body,
            status: switch (skills.where((s) => s.name == entry.name).firstOrNull) {
              null => 'notInstalled',
              _ when outdated.contains(entry.name) => 'outdated',
              final found when found.description == entry.description && found.body == entry.body && !found.proposed => 'installed',
              _ => 'modified',
            },
          ),
      ];

  @override
  Future<void> installCatalog(String name, {required bool overwrite}) async {
    final entry = gallery.firstWhere((s) => s.name == name);
    final index = skills.indexWhere((s) => s.name == name);
    if (index >= 0 && !overwrite && !outdated.contains(name)) throw 'a skill named $name is already in your skills and differs from this one';
    outdated.remove(name);
    if (index >= 0) {
      skills[index] = entry;
    } else {
      skills.add(entry);
    }
  }

  @override
  Future<List<SkillDto>> list() async => List.of(skills);

  @override
  Future<void> save(SkillDto skill, {required bool overwrite}) async {
    if (skill.name.isEmpty) throw 'skill name must be 1-64 characters';
    final index = skills.indexWhere((s) => s.name == skill.name);
    if (index >= 0 && !overwrite) throw "a skill named '${skill.name}' already exists";
    if (index >= 0) {
      skills[index] = skill;
    } else {
      skills.add(skill);
    }
  }

  @override
  Future<void> delete(String name) async => skills.removeWhere((s) => s.name == name);
}

/// The hub's model, as `SkillFormScreen` sees it: answers with [draft], or refuses with [error]. Records what it was asked.
class _FakeDraftBackend implements SkillDraftBackend {
  _FakeDraftBackend(SkillDraft this.draft) : error = null;
  _FakeDraftBackend.failing(String this.error) : draft = null;

  final SkillDraft? draft;
  final String? error;
  final asked = <String>[];

  @override
  Future<SkillDraft> draftSkill(String description) async {
    asked.add(description);
    if (error != null) throw ConversationException(error!);
    return draft!;
  }
}

const _review = SkillDto(name: 'review-pr', description: 'Reviews a PR', body: 'Read the diff.', proposed: false);
const _tip = SkillDto(name: 'tip', description: 'A tip', body: 'Do it.', proposed: true, source: 'a conversa');
const _change = SkillDto(name: 'review-pr-revision', description: 'Reviews a PR', body: 'Check tests first.', proposed: true, revises: 'review-pr');

Future<void> _pump(WidgetTester tester, _FakeRepository repo) async {
  await tester.pumpWidget(testApp(home: SkillsScreen(repository: repo)));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('shows an empty state when there are no skills', (tester) async {
    await _pump(tester, _FakeRepository([]));
    expect(find.textContaining('No skills yet'), findsOneWidget);
  });

  testWidgets('lists the skills with their descriptions', (tester) async {
    await _pump(tester, _FakeRepository([_review]));
    expect(find.text('review-pr'), findsOneWidget);
    expect(find.text('Reviews a PR'), findsOneWidget);
  });

  testWidgets('creating a skill saves it and returns to the list', (tester) async {
    final repo = _FakeRepository([]);
    await _pump(tester, repo);

    await tester.tap(find.text('New skill'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Name'), 'summarize');
    await tester.enterText(find.widgetWithText(TextField, 'Description'), 'Summarizes text');
    await tester.enterText(find.widgetWithText(TextField, 'Instructions'), 'Be brief.');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(repo.skills.single.name, 'summarize');
    expect(repo.skills.single.body, 'Be brief.');
    expect(find.text('summarize'), findsOneWidget);
  });

  testWidgets('a rejected save stays on the form and shows the error', (tester) async {
    await _pump(tester, _FakeRepository([_review]));

    await tester.tap(find.text('New skill'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Name'), 'review-pr');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(find.textContaining('already exists'), findsOneWidget);
    expect(find.text('New skill'), findsWidgets);
  });

  testWidgets('editing locks the name and overwrites the skill', (tester) async {
    final repo = _FakeRepository([_review]);
    await _pump(tester, repo);

    await tester.tap(find.text('review-pr'));
    await tester.pumpAndSettle();
    expect(tester.widget<TextField>(find.widgetWithText(TextField, 'Name')).enabled, isFalse);

    await tester.enterText(find.widgetWithText(TextField, 'Description'), 'Reviews a pull request');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(repo.skills.single.description, 'Reviews a pull request');
  });

  testWidgets('delete asks for confirmation and only removes on confirm', (tester) async {
    final repo = _FakeRepository([_review]);
    await _pump(tester, repo);

    await tester.tap(find.byTooltip('Delete review-pr'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(repo.skills, hasLength(1));

    await tester.tap(find.byTooltip('Delete review-pr'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Delete'));
    await tester.pumpAndSettle();
    expect(repo.skills, isEmpty);
    expect(find.textContaining('No skills yet'), findsOneWidget);
  });

  testWidgets('a suggestion gets its own section and stays pending when edited and saved', (tester) async {
    final repo = _FakeRepository([_review, _tip]);
    await _pump(tester, repo);
    expect(find.text('Suggested by the AI'), findsOneWidget);
    expect(find.text('Suggested'), findsOneWidget);

    await tester.tap(find.text('Edit'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(repo.skills.firstWhere((s) => s.name == 'tip').proposed, isTrue);
  });

  testWidgets('accepting a suggestion saves it as active', (tester) async {
    final repo = _FakeRepository([_tip]);
    await _pump(tester, repo);

    await tester.tap(find.text('Accept'));
    await tester.pumpAndSettle();

    expect(repo.skills.single.proposed, isFalse);
    expect(find.text('Suggested by the AI'), findsNothing);
  });

  testWidgets('rejecting a suggestion deletes it after confirming', (tester) async {
    final repo = _FakeRepository([_tip]);
    await _pump(tester, repo);

    await tester.tap(find.text('Reject'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Reject')));
    await tester.pumpAndSettle();

    expect(repo.skills, isEmpty);
  });

  group('gallery', () {
    const emailDraft = SkillDto(name: 'email-draft', description: 'Draft an email', body: 'Be polite.', proposed: false);
    const explain = SkillDto(name: 'explain-simply', description: 'Explain it', body: 'Use plain words.', proposed: false);

    testWidgets('is left out when the bridge has no gallery', (tester) async {
      await _pump(tester, _FakeRepository([_review]));
      expect(find.text('Gallery'), findsNothing);
    });

    testWidgets('shows on an empty vault too, and installing copies the skill into it', (tester) async {
      final repo = _FakeRepository([], gallery: [emailDraft, explain]);
      await _pump(tester, repo);
      expect(find.textContaining('No skills yet'), findsOneWidget);
      expect(find.text('Gallery'), findsOneWidget);
      expect(find.text('email-draft'), findsOneWidget);
      expect(find.text('Install'), findsNWidgets(2));

      await tester.tap(find.text('Install').first);
      await tester.pumpAndSettle();

      expect(repo.skills.single.name, 'email-draft');
      expect(find.text('Installed'), findsOneWidget);
      expect(find.text('Install'), findsOneWidget);
      expect(find.textContaining('No skills yet'), findsNothing);
    });

    testWidgets('an entry that matches the vault says Installed and offers nothing', (tester) async {
      await _pump(tester, _FakeRepository([emailDraft], gallery: [emailDraft]));
      expect(find.text('Installed'), findsOneWidget);
      expect(find.text('Install'), findsNothing);
    });

    testWidgets('an untouched skill with newer gallery text offers Update, which replaces it without asking twice', (tester) async {
      const old = SkillDto(name: 'email-draft', description: 'Draft an email', body: 'Old steps.', proposed: false);
      final repo = _FakeRepository([old], gallery: [emailDraft], outdated: {'email-draft'});
      await _pump(tester, repo);
      expect(find.text('A newer version is available; yours is untouched'), findsOneWidget);
      expect(find.text('Update'), findsOneWidget);
      expect(find.text('Install'), findsNothing);
      expect(find.text('You have another skill with this name'), findsNothing);

      await tester.tap(find.text('Update'));
      await tester.pumpAndSettle();

      expect(repo.skills.single.body, 'Be polite.');
      expect(find.text('A newer version is available; yours is untouched'), findsNothing);
      expect(find.text('Installed'), findsOneWidget);
    });

    testWidgets('a skill with the same name that differs is only replaced after a second, explicit tap', (tester) async {
      const mine = SkillDto(name: 'email-draft', description: 'Draft an email', body: 'My own steps.', proposed: false);
      final repo = _FakeRepository([mine], gallery: [emailDraft]);
      await _pump(tester, repo);
      expect(find.text('You have another skill with this name'), findsOneWidget);
      expect(find.text('Install'), findsNothing);

      await tester.tap(find.text("Use the gallery's"));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Keep mine'));
      await tester.pumpAndSettle();
      expect(repo.skills.single.body, 'My own steps.');
      expect(find.text('You have another skill with this name'), findsOneWidget);

      await tester.tap(find.text("Use the gallery's"));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Replace mine'));
      await tester.pumpAndSettle();
      expect(repo.skills.single.body, 'Be polite.');
      expect(find.text('Installed'), findsOneWidget);
    });

    testWidgets('describes the entry in Portuguese when the app is, and falls back to the bridge text for a new one', (tester) async {
      const fresh = SkillDto(name: 'brand-new', description: 'Hub text only', body: 'x', proposed: false);
      final repo = _FakeRepository([], gallery: [emailDraft, fresh]);
      await tester.pumpWidget(testApp(home: SkillsScreen(repository: repo), locale: const Locale('pt')));
      await tester.pumpAndSettle();
      expect(find.text('Galeria'), findsOneWidget);
      expect(find.textContaining('Redigir um e-mail claro'), findsOneWidget);
      expect(find.text('Hub text only'), findsOneWidget);
      expect(find.text('Instalar'), findsNWidgets(2));
    });

    testWidgets('a failed install shows the error', (tester) async {
      final repo = _FakeRepository([], gallery: [emailDraft]);
      await _pump(tester, repo);
      repo.gallery.clear();
      await tester.tap(find.text('Install'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Failed to install the skill'), findsOneWidget);
    });
  });

  testWidgets('a suggested change to a skill says which skill it changes', (tester) async {
    await _pump(tester, _FakeRepository([_review, _change]));
    expect(find.text('Change to review-pr'), findsOneWidget);
    expect(find.text('See what changes in review-pr'), findsOneWidget);
    await tester.tap(find.text('See what changes in review-pr'));
    await tester.pumpAndSettle();
    expect(find.text('- Read the diff.'), findsOneWidget);
    expect(find.text('+ Check tests first.'), findsOneWidget);
    expect(find.text('Suggested'), findsNothing);
  });

  group('draft by the hub (P72)', () {
    const draftLabel = 'Describe the skill and the AI writes a draft';

    Future<void> openNewForm(WidgetTester tester, _FakeRepository repo, SkillDraftBackend? hub) async {
      await tester.pumpWidget(testApp(home: SkillsScreen(repository: repo, draftBackend: hub)));
      await tester.pumpAndSettle();
      await tester.tap(find.text('New skill'));
      await tester.pumpAndSettle();
    }

    testWidgets('fills the form from the description and saves nothing', (tester) async {
      final repo = _FakeRepository([]);
      final hub = _FakeDraftBackend(const SkillDraft(name: 'pr-summary', description: 'Summarise PRs', body: '1. List the PRs'));
      await openNewForm(tester, repo, hub);

      await tester.enterText(find.widgetWithText(TextField, draftLabel), 'summarise my PRs');
      await tester.pump(); // the button enables on the next frame
      await tester.tap(find.text('Write a draft'));
      await tester.pumpAndSettle();

      expect(hub.asked, ['summarise my PRs']);
      expect(tester.widget<TextField>(find.widgetWithText(TextField, 'Name')).controller!.text, 'pr-summary');
      expect(tester.widget<TextField>(find.widgetWithText(TextField, 'Description')).controller!.text, 'Summarise PRs');
      expect(tester.widget<TextField>(find.widgetWithText(TextField, 'Instructions')).controller!.text, '1. List the PRs');
      expect(repo.skills, isEmpty);
    });

    testWidgets('the hub refusing the draft shows its text and leaves the form as it was', (tester) async {
      final repo = _FakeRepository([]);
      final hub = _FakeDraftBackend.failing('spending limit reached');
      await openNewForm(tester, repo, hub);

      await tester.enterText(find.widgetWithText(TextField, 'Name'), 'mine');
      await tester.enterText(find.widgetWithText(TextField, draftLabel), 'anything');
      await tester.pump(); // the button enables on the next frame
      await tester.tap(find.text('Write a draft'));
      await tester.pumpAndSettle();

      expect(find.text('Failed to write the draft: spending limit reached'), findsOneWidget);
      expect(tester.widget<TextField>(find.widgetWithText(TextField, 'Name')).controller!.text, 'mine');
      expect(repo.skills, isEmpty);
    });

    testWidgets('the button waits for a description', (tester) async {
      await openNewForm(tester, _FakeRepository([]), _FakeDraftBackend(const SkillDraft(name: 'a', description: 'b', body: 'c')));
      expect(tester.widget<OutlinedButton>(find.ancestor(of: find.text('Write a draft'), matching: find.byType(OutlinedButton))).onPressed, isNull);
    });

    testWidgets('is left out with no hub connection, and when editing', (tester) async {
      await openNewForm(tester, _FakeRepository([]), null);
      expect(find.text(draftLabel), findsNothing);

      final hub = _FakeDraftBackend(const SkillDraft(name: 'a', description: 'b', body: 'c'));
      await tester.pumpWidget(testApp(home: SkillFormScreen(repository: _FakeRepository([_review]), existing: _review, draftBackend: hub)));
      await tester.pumpAndSettle();
      expect(find.text(draftLabel), findsNothing);
    });
  });
}
