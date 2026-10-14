import '../protocol/messages.dart' show PendingDeviceInvite;

/// A one-use invite the hub made for a new device (P36). The [code] is shown once: the hub keeps only its hash.
class DeviceInvite {
  const DeviceInvite({required this.name, required this.code, required this.expiresMs});

  final String name;
  final String code;
  final int expiresMs;
}

/// What the Devices screen asks of the hub (P36), so the screen can be tested without one. Every call but the list takes the
/// hub's pairing key, asked every time and never kept, and throws a `HubRequestException` (`authRejected` on a wrong key).
abstract interface class DeviceInvitesBackend {
  /// The invites that wait to be used, by name; the hub never says their codes. Open to a paired device.
  Future<List<PendingDeviceInvite>> listDeviceInvites();

  /// Cancels the waiting invite called [name] and returns the ones that still wait.
  Future<List<PendingDeviceInvite>> cancelDeviceInvite(String pairingKey, String name);

  /// A one-use invite called [name] that pairs one device without the pairing key, valid for [ttlMinutes].
  Future<DeviceInvite> createDeviceInvite(String pairingKey, String name, int ttlMinutes);

  /// Changes the hub's pairing key and returns the new one (shown once). Devices that already hold a token keep working.
  Future<String> rotatePairingKey(String pairingKey);
}
