import '../src/rust/api/skills.dart' as bridge;
import '../src/rust/api/skills.dart' show CatalogSkillDto, SkillDto;
import 'vault_paths.dart';

export '../src/rust/api/skills.dart' show CatalogSkillDto, SkillDto;

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
  /// `installed` or `modified`.
  Future<List<CatalogSkillDto>> listCatalog();

  /// Copies the gallery skill `name` into the vault. One that differs from what's there is only
  /// replaced with `overwrite: true`.
  Future<void> installCatalog(String name, {required bool overwrite});
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
}
