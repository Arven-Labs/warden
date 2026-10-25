import 'package:url_launcher/url_launcher.dart';

/// Opens an address outside the app. Injected into the screens so a test can use a fake instead of the platform plugin.
typedef UrlLauncher = Future<bool> Function(Uri uri);

/// The system browser (or whatever app handles the address), not a view inside Warden.
Future<bool> launchExternal(Uri uri) => launchUrl(uri, mode: LaunchMode.externalApplication);

/// P168 — the address to buy more hosted credit, if the hub sent a usable one: it is handed to the system, so only a plain `https://`
/// address with a host and no login in it passes (a hub is trusted with the chat, not with opening a `tel:` or `intent:` link).
/// Anything else is "no link", and the balance chip and the error action open the usage screen instead.
Uri? topUpUri(String? raw) {
  if (raw == null || raw.isEmpty) return null;
  final uri = Uri.tryParse(raw);
  if (uri == null || uri.scheme != 'https' || uri.host.isEmpty || uri.userInfo.isNotEmpty) return null;
  return uri;
}
