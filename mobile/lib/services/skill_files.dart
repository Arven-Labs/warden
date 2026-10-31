import 'dart:convert';

import 'skills_repository.dart' show SkillFileInfo;

// P72 d — the checks done on a skill's attached file before it is sent. They mirror the Rust store's
// (`warden_core::skill::validate_file_name`, 64 KB, 20 per skill), which is the real barrier: this only
// lets a bad file fail without a round trip, and say why in the app's language.

const maxFileBytes = 64 * 1024;
const maxFilesPerSkill = 20;
const maxFileNameLen = 64;

enum FileNameProblem { length, chars, dot }

enum ContentProblem { tooBig, notText }

/// Why [file] can't be an attachment's name, or null when it can: 1 to 64 of `[A-Za-z0-9._-]`, not starting with a dot.
FileNameProblem? validateFileName(String file) {
  if (file.isEmpty || file.length > maxFileNameLen) return FileNameProblem.length;
  if (!RegExp(r'^[A-Za-z0-9._-]+$').hasMatch(file)) return FileNameProblem.chars;
  if (file.startsWith('.')) return FileNameProblem.dot;
  return null;
}

/// Why [text] can't be an attachment, or null when it can: more than 64 KB (counted in bytes, as the store does, not in
/// characters) or holding a NUL.
ContentProblem? checkContent(String text) {
  if (utf8.encode(text).length > maxFileBytes) return ContentProblem.tooBig;
  if (text.contains('\u0000')) return ContentProblem.notText;
  return null;
}

/// Whether [name] can be saved on a skill that has [files] already: replacing one always fits, a new name only below twenty.
bool canAttach(List<SkillFileInfo> files, String name) => files.any((f) => f.name == name) || files.length < maxFilesPerSkill;
