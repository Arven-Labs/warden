import 'package:flutter_test/flutter_test.dart';
import 'package:mobile/services/skill_files.dart';
import 'package:mobile/services/skills_repository.dart' show SkillFileInfo;

// P72 d — the checks done on a skill's attached file before it is sent. They mirror the Rust store's (the real barrier), so a bad file fails fast.
void main() {
  group('an attachment name', () {
    test('letters, digits, dot, dash and underscore are fine', () {
      for (final ok in ['notes.md', 'a', 'My_File-2.txt', 'x' * 64]) {
        expect(validateFileName(ok), isNull, reason: ok);
      }
    });

    test('the problem says which rule it broke', () {
      expect(validateFileName(''), FileNameProblem.length);
      expect(validateFileName('x' * 65), FileNameProblem.length);
      expect(validateFileName('a/b'), FileNameProblem.chars);
      expect(validateFileName('.hidden'), FileNameProblem.dot);
    });

    test('anything that could leave the skill folder or hide is refused', () {
      for (final bad in ['../x.md', 'a/b.md', 'a\\b.md', '.hidden', '..', '', 'nul\u0000.md', 'sp ace.md', 'é.md', 'x' * 65]) {
        expect(validateFileName(bad), isNotNull, reason: bad);
      }
    });
  });

  group('an attachment text', () {
    test('the limit is in bytes, not characters', () {
      expect(checkContent('a' * maxFileBytes), isNull);
      expect(checkContent('a' * (maxFileBytes + 1)), ContentProblem.tooBig);
      // "é" is 2 bytes: 32 769 of them are 32 769 characters but 65 538 bytes.
      expect(checkContent('é' * (maxFileBytes ~/ 2)), isNull);
      expect(checkContent('é' * (maxFileBytes ~/ 2 + 1)), ContentProblem.tooBig);
    });

    test('a NUL is not text, an empty file is', () {
      expect(checkContent('a\u0000b'), ContentProblem.notText);
      expect(checkContent(''), isNull);
    });
  });

  group('how many attachments fit', () {
    final twenty = [for (var i = 0; i < maxFilesPerSkill; i++) SkillFileInfo(name: 'f$i.md', size: 1)];

    test('a new name fits until there are twenty', () {
      expect(canAttach(twenty.sublist(1), 'new.md'), isTrue);
      expect(canAttach(twenty, 'new.md'), isFalse);
    });

    test('replacing one of the twenty is still fine', () {
      expect(canAttach(twenty, 'f3.md'), isTrue);
    });
  });
}
