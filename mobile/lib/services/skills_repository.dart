import '../src/rust/api/skills.dart' as bridge;
import '../src/rust/api/skills.dart' show CatalogSkillDto, SkillDto, SkillFileInfo;
import 'vault_paths.dart';

export '../src/rust/api/skills.dart' show CatalogSkillDto, SkillDto, SkillFileInfo;

/// P72 (b) — the seam between `SkillsScreen` and the Rust bridge, so the screen can be tested with
/// a fake instead of a native library. The real implementation reads/writes `skills/<name>.md` in
/// the local vault through `warden_core::skill` (same validation as the desktop/CLI), which is why
/// the name/description/body rules aren't repeated on the Dart side: a rejected skill comes back as
/// an exception whose message is the Rust error.
abstract class SkillsRepository {
  Future<List<SkillDto>> list();

  /// `overwrite: false` creates (refuses a name already taken); `true` edits.
  Future<void> save(SkillDto skill, {required bool overwrite});

  Future<void> delete(String name);

  /// The gallery of ready-made skills (P140), each with its `status` in this vault: `notInstalled`,
  /// `installed`, `outdated` (untouched, but the gallery has newer text) or `modified`.
  Future<List<CatalogSkillDto>> listCatalog();

  /// Copies the gallery skill `name` into the vault. One that differs from what's there is only
  /// replaced with `overwrite: true`.
  Future<void> installCatalog(String name, {required bool overwrite});

  /// The skill's attached files (P72 d), by name and size in bytes, sorted. They sit in `skills/<name>.files/`, next to the
  /// skill, and ride the vault sync with it. Empty when there are none.
  Future<List<SkillFileInfo>> listFiles(String name);

  Future<String> readFile(String name, String file);

  /// Creates or replaces an attachment; the skill must already be saved. The Rust store refuses a bad name, more than
  /// 64 KB and a 21st file.
  Future<void> saveFile(String name, String file, String content);

  Future<void> deleteFile(String name, String file);
}

class BridgeSkillsRepository implements SkillsRepository {
  const BridgeSkillsRepository();

  Future<String> _vaultRoot() async => (await VaultPaths.resolve()).vaultRoot;

  @override
  Future<List<SkillDto>> list() async => bridge.bridgeListSkills(vaultRoot: await _vaultRoot());

  @override
  Future<void> save(SkillDto skill, {required bool overwrite}) async =>
      bridge.bridgeSaveSkill(vaultRoot: await _vaultRoot(), skill: skill, overwrite: overwrite);

  @override
  Future<void> delete(String name) async => bridge.bridgeDeleteSkill(vaultRoot: await _vaultRoot(), name: name);

  @override
  Future<List<CatalogSkillDto>> listCatalog() async => bridge.bridgeListSkillCatalog(vaultRoot: await _vaultRoot());

  @override
  Future<void> installCatalog(String name, {required bool overwrite}) async =>
      bridge.bridgeInstallCatalogSkill(vaultRoot: await _vaultRoot(), name: name, overwrite: overwrite);

  @override
  Future<List<SkillFileInfo>> listFiles(String name) async => bridge.bridgeListSkillFiles(vaultRoot: await _vaultRoot(), name: name);

  @override
  Future<String> readFile(String name, String file) async =>
      bridge.bridgeReadSkillFile(vaultRoot: await _vaultRoot(), name: name, file: file);

  @override
  Future<void> saveFile(String name, String file, String content) async =>
      bridge.bridgeSaveSkillFile(vaultRoot: await _vaultRoot(), name: name, file: file, content: content);

  @override
  Future<void> deleteFile(String name, String file) async =>
      bridge.bridgeDeleteSkillFile(vaultRoot: await _vaultRoot(), name: name, file: file);
}
