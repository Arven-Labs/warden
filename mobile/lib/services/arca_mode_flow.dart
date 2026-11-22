import '../l10n/generated/app_localizations.dart';
import '../src/rust/api/arca_mode.dart' as bridge;
import 'vault_paths.dart';

/// What the arca mode screen needs from the Rust bridge, so a test can stand in for it (tests never load `RustLib`).
abstract class ArcaModeBackend {
  Future<bridge.ArcaModeStatusDto> status();

  /// Turns the mode on. Throws `bridge.ArcaModeFailure`.
  Future<void> enable();

  /// One cycle. `allowInsecureHttp` only after the person was warned; the bridge still refuses a public address.
  Future<bridge.ArcaSyncDto> syncRun({required String hubUrl, required String deviceToken, required bool allowInsecureHttp});

  Future<List<bridge.PendingEpochDto>> epochsPending();

  /// How many of [seen] were acknowledged. 0 means the list changed under the person.
  Future<int> epochsAcknowledge(List<bridge.PendingEpochDto> seen);
}

/// The sentence for a failure kind, in the app's language (the Rust text is English and never shown as the main message).
String arcaModeErrorText(AppLocalizations l10n, bridge.ArcaModeErrorKind kind) {
  throw UnimplementedError('tests first');
}

/// The first hex digits of a manifest id (`manifest/<hex>`), enough for a person to tell two apart (never used to look anything up).
String epochShortId(String id) {
  throw UnimplementedError('tests first');
}

/// The real backend: the Rust bridge, over the same four sync paths the Sync screen uses.
class BridgeArcaModeBackend implements ArcaModeBackend {
  @override
  Future<bridge.ArcaModeStatusDto> status() async {
    final paths = await VaultPaths.resolve();
    return bridge.arcaModeStatus(vaultRoot: paths.vaultRoot, configPath: paths.configPath, secretsPath: paths.secretsPath, manifestPath: paths.manifestPath);
  }

  @override
  Future<void> enable() async {
    final paths = await VaultPaths.resolve();
    await bridge.arcaModeEnable(
      vaultRoot: paths.vaultRoot,
      configPath: paths.configPath,
      secretsPath: paths.secretsPath,
      manifestPath: paths.manifestPath,
      nowMs: BigInt.from(DateTime.now().millisecondsSinceEpoch),
    );
  }

  @override
  Future<bridge.ArcaSyncDto> syncRun({required String hubUrl, required String deviceToken, required bool allowInsecureHttp}) async {
    final paths = await VaultPaths.resolve();
    return bridge.arcaSyncRun(
      vaultRoot: paths.vaultRoot,
      configPath: paths.configPath,
      secretsPath: paths.secretsPath,
      manifestPath: paths.manifestPath,
      hubUrl: hubUrl,
      deviceToken: deviceToken,
      allowInsecureHttp: allowInsecureHttp,
      nowMs: BigInt.from(DateTime.now().millisecondsSinceEpoch),
    );
  }

  @override
  Future<List<bridge.PendingEpochDto>> epochsPending() async {
    final paths = await VaultPaths.resolve();
    return bridge.arcaEpochsPending(vaultRoot: paths.vaultRoot, configPath: paths.configPath, secretsPath: paths.secretsPath, manifestPath: paths.manifestPath);
  }

  @override
  Future<int> epochsAcknowledge(List<bridge.PendingEpochDto> seen) async {
    final paths = await VaultPaths.resolve();
    return bridge.arcaEpochsAcknowledge(vaultRoot: paths.vaultRoot, configPath: paths.configPath, secretsPath: paths.secretsPath, manifestPath: paths.manifestPath, seen: seen);
  }
}
