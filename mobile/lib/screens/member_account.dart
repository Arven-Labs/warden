import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../l10n/generated/app_localizations.dart';
import '../protocol/messages.dart';
import '../services/server_connection.dart';

// P84 slice 4 and 5 on the phone: what a member sees about their own data. Everything here mirrors
// the web's `ChangePasswordView`, `RecoveryCodeView` and `RecoveryNoticeView`, and talks to the same
// hub messages.

const minPasswordLength = 8;

/// Shows a recovery code once. There's no way to dismiss it before "I wrote it down" is ticked: the
/// code is the only way back to the data after a forgotten password, and the hub can't show it again.
Future<void> showRecoveryCode(BuildContext context, String code, {bool replacing = false}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (_) => PopScope(canPop: false, child: RecoveryCodeDialog(code: code, replacing: replacing)),
  );
}

class RecoveryCodeDialog extends StatefulWidget {
  const RecoveryCodeDialog({super.key, required this.code, this.replacing = false});

  final String code;

  /// A code the member asked for (or a new policy made): the old one stopped working.
  final bool replacing;

  @override
  State<RecoveryCodeDialog> createState() => _RecoveryCodeDialogState();
}

class _RecoveryCodeDialogState extends State<RecoveryCodeDialog> {
  bool _saved = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.replacing ? l10n.acctCodeNewTitle : l10n.acctCodeTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(widget.replacing ? l10n.acctCodeBodyNew : l10n.acctCodeBody),
            const SizedBox(height: 12),
            SelectableText(widget.code, key: const Key('recovery-code'), style: const TextStyle(fontFamily: 'monospace', fontSize: 16, fontWeight: FontWeight.w600)),
            TextButton.icon(
              onPressed: () => Clipboard.setData(ClipboardData(text: widget.code)),
              icon: const Icon(Icons.copy, size: 18),
              label: Text(l10n.commonCopy),
            ),
            Text(l10n.acctCodeOnce),
            CheckboxListTile(
              key: const Key('recovery-code-saved'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: _saved,
              onChanged: (value) => setState(() => _saved = value ?? false),
              title: Text(l10n.acctCodeSaved),
            ),
          ],
        ),
      ),
      actions: [
        FilledButton(onPressed: _saved ? () => Navigator.of(context).pop() : null, child: Text(l10n.acctContinue)),
      ],
    );
  }
}

/// What a password change gave back: the recovery code, when it turned encryption on.
class PasswordChange {
  const PasswordChange({this.recoveryCode});

  final String? recoveryCode;
}

/// Swaps the provisional password for the member's own (first sign-in, [required]) or changes it later.
/// [needsRecovery]: the owner reset the password of someone whose data is encrypted, so the recovery
/// code is asked for too. Pops a [PasswordChange] once the hub took it, null if they cancel.
class ChangePasswordDialog extends StatefulWidget {
  const ChangePasswordDialog({super.key, required this.connection, required this.name, this.required = true, this.needsRecovery = false});

  final ServerConnection connection;
  final String name;
  final bool required;
  final bool needsRecovery;

  @override
  State<ChangePasswordDialog> createState() => _ChangePasswordDialogState();
}

class _ChangePasswordDialogState extends State<ChangePasswordDialog> {
  final _current = TextEditingController();
  final _next = TextEditingController();
  final _again = TextEditingController();
  final _code = TextEditingController();
  String? _error;
  bool _busy = false;

  Future<void> _submit() async {
    final l10n = AppLocalizations.of(context);
    if (_next.text.length < minPasswordLength) {
      setState(() => _error = l10n.acctPasswordShort(minPasswordLength));
      return;
    }
    if (_next.text != _again.text) {
      setState(() => _error = l10n.acctPasswordMismatch);
      return;
    }
    if (widget.needsRecovery && _code.text.trim().isEmpty) {
      setState(() => _error = l10n.acctNeedCode);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final code = await widget.connection.changePassword(_current.text, _next.text, recoveryCode: widget.needsRecovery ? _code.text.trim() : null);
      if (mounted) Navigator.of(context).pop(PasswordChange(recoveryCode: code));
    } on PasswordException catch (e) {
      if (mounted) setState(() => _error = e.wrongPassword ? (widget.required ? l10n.acctWrongProvisional : l10n.acctWrongCurrent) : e.message);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _current.dispose();
    _next.dispose();
    _again.dispose();
    _code.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.required ? l10n.acctChooseTitle : l10n.acctChangeTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (widget.required) Text(l10n.acctHi(widget.name)),
            TextField(controller: _current, obscureText: true, decoration: InputDecoration(labelText: widget.required ? l10n.acctProvisional : l10n.acctCurrent)),
            if (widget.needsRecovery)
              TextField(
                key: const Key('change-password-recovery-code'),
                controller: _code,
                autocorrect: false,
                textCapitalization: TextCapitalization.characters,
                decoration: InputDecoration(
                  labelText: l10n.acctRecoveryCode,
                  helperText: l10n.acctRecoveryHelper,
                  helperMaxLines: 3,
                ),
              ),
            TextField(controller: _next, obscureText: true, decoration: InputDecoration(labelText: l10n.acctNewPassword)),
            TextField(controller: _again, obscureText: true, decoration: InputDecoration(labelText: l10n.acctNewPasswordAgain)),
            if (_error != null) Padding(padding: const EdgeInsets.only(top: 12), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(onPressed: _busy ? null : _submit, child: Text(l10n.commonSave)),
      ],
    );
  }
}

/// The workspace's recovery policy changed to a weaker one (P84 slice 4 parte B): their data follows it
/// only if they say yes, with their password. Pops the new recovery code (or an empty string when there
/// isn't one) once accepted, null for "Not now".
class AcceptPolicyDialog extends StatefulWidget {
  const AcceptPolicyDialog({super.key, required this.connection, required this.policy});

  final ServerConnection connection;

  /// The workspace's policy: `consent` or `company`.
  final String policy;

  @override
  State<AcceptPolicyDialog> createState() => _AcceptPolicyDialogState();
}

class _AcceptPolicyDialogState extends State<AcceptPolicyDialog> {
  final _password = TextEditingController();
  String? _error;
  bool _busy = false;

  Future<void> _accept() async {
    final l10n = AppLocalizations.of(context);
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final code = await widget.connection.acceptRecoveryPolicy(_password.text);
      if (mounted) Navigator.of(context).pop(code ?? '');
    } on PasswordException catch (e) {
      if (mounted) setState(() => _error = e.wrongPassword ? l10n.acctWrongThat : e.message);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final company = widget.policy == 'company';
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.acctPolicyTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(company ? l10n.acctPolicyCompany : l10n.acctPolicyConsent),
            const SizedBox(height: 8),
            Text(l10n.acctPolicyToday),
            const SizedBox(height: 8),
            Text(l10n.acctPolicyKeep),
            TextField(key: const Key('accept-policy-password'), controller: _password, obscureText: true, onChanged: (_) => setState(() {}), decoration: InputDecoration(labelText: l10n.acctPolicyPassword)),
            if (_error != null) Padding(padding: const EdgeInsets.only(top: 12), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.of(context).pop(), child: Text(l10n.acctNotNow)),
        FilledButton(onPressed: _busy || _password.text.isEmpty ? null : _accept, child: Text(l10n.acctAccept)),
      ],
    );
  }
}

/// The owner recovered their data (P84 slice 4 parte B): they're told, once.
class RecoveryNoticeDialog extends StatelessWidget {
  const RecoveryNoticeDialog({super.key, required this.events});

  final List<RecoveryEvent> events;

  static String _when(int atMs) {
    final t = DateTime.fromMillisecondsSinceEpoch(atMs).toLocal();
    String two(int n) => n.toString().padLeft(2, '0');
    return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}';
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.acctRecoveredTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.acctRecoveredBody),
            const SizedBox(height: 8),
            for (final event in events) Text('${_when(event.atMs)} — ${event.kind == 'company' ? l10n.acctRecoveredCompany : l10n.acctRecoveredConsent}'),
            const SizedBox(height: 8),
            Text(l10n.acctRecoveredNotYou),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(false), child: Text(l10n.acctNotNow)),
        FilledButton(onPressed: () => Navigator.of(context).pop(true), child: Text(l10n.acctGotIt)),
      ],
    );
  }
}

/// A member's own account: password, recovery code and TruthID. Pushed from the connection screen.
class AccountScreen extends StatefulWidget {
  const AccountScreen({super.key, required this.connection});

  final ServerConnection connection;

  @override
  State<AccountScreen> createState() => _AccountScreenState();
}

class _AccountScreenState extends State<AccountScreen> {
  String? _truthid;

  @override
  void initState() {
    super.initState();
    final user = widget.connection.user;
    _truthid = user != null && user.truthid.isNotEmpty ? user.truthid : null;
  }

  Future<void> _changePassword() async {
    final user = widget.connection.user;
    final change = await showDialog<PasswordChange>(
      context: context,
      builder: (_) => ChangePasswordDialog(connection: widget.connection, name: user?.name ?? '', required: false),
    );
    final code = change?.recoveryCode;
    if (code != null && mounted) await showRecoveryCode(context, code);
  }

  Future<void> _newCode() async {
    final l10n = AppLocalizations.of(context);
    final password = await showDialog<String>(context: context, builder: (_) => _AskPasswordDialog(title: l10n.acctNewCodeTitle, hint: l10n.acctNewCodeHint));
    if (password == null || !mounted) return;
    try {
      final code = await widget.connection.regenerateRecoveryCode(password);
      if (mounted) await showRecoveryCode(context, code, replacing: true);
    } on PasswordException catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.wrongPassword ? l10n.acctWrongThat : e.message)));
    }
  }

  Future<void> _linkTruthId() async {
    final linked = await showDialog<String>(context: context, builder: (_) => _LinkTruthIdDialog(connection: widget.connection));
    if (linked != null && mounted) setState(() => _truthid = linked);
  }

  @override
  Widget build(BuildContext context) {
    final user = widget.connection.user;
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(user?.name ?? l10n.acctTitle)),
      body: ListView(
        children: [
          ListTile(leading: const Icon(Icons.person), title: Text(user?.id ?? ''), subtitle: Text(user?.encrypted ?? false ? l10n.acctEncrypted : l10n.acctNotEncrypted)),
          ListTile(key: const Key('account-change-password'), leading: const Icon(Icons.lock_reset), title: Text(l10n.acctChangePassword), onTap: _changePassword),
          if (user?.encrypted ?? false) ListTile(key: const Key('account-new-code'), leading: const Icon(Icons.key), title: Text(l10n.acctGetNewCode), subtitle: Text(l10n.acctOldStops), onTap: _newCode),
          ListTile(
            key: const Key('account-link-truthid'),
            leading: const Icon(Icons.verified_user),
            title: Text(_truthid == null ? l10n.acctLinkTruthid : l10n.acctTruthidIs(_truthid!)),
            subtitle: Text(_truthid == null ? l10n.acctLinkHint : l10n.acctLinkedHint),
            onTap: _truthid == null ? _linkTruthId : null,
          ),
        ],
      ),
    );
  }
}

class _AskPasswordDialog extends StatefulWidget {
  const _AskPasswordDialog({required this.title, required this.hint});

  final String title;
  final String hint;

  @override
  State<_AskPasswordDialog> createState() => _AskPasswordDialogState();
}

class _AskPasswordDialogState extends State<_AskPasswordDialog> {
  final _password = TextEditingController();

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(widget.title),
      content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(widget.hint),
        TextField(controller: _password, obscureText: true, autofocus: true, decoration: InputDecoration(labelText: l10n.acctPassword)),
      ]),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(onPressed: () => Navigator.of(context).pop(_password.text), child: Text(l10n.acctContinue)),
      ],
    );
  }
}

class _LinkTruthIdDialog extends StatefulWidget {
  const _LinkTruthIdDialog({required this.connection});

  final ServerConnection connection;

  @override
  State<_LinkTruthIdDialog> createState() => _LinkTruthIdDialogState();
}

class _LinkTruthIdDialogState extends State<_LinkTruthIdDialog> {
  final _code = TextEditingController();
  final _username = TextEditingController();
  String? _error;
  bool _busy = false;

  Future<void> _link() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final linked = await widget.connection.redeemInvite(_code.text.trim(), _username.text.trim());
      if (mounted) Navigator.of(context).pop(linked);
    } on PasswordException catch (e) {
      if (mounted) setState(() => _error = e.message);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _code.dispose();
    _username.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.acctLinkTruthid),
      content: SingleChildScrollView(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          TextField(key: const Key('truthid-invite-code'), controller: _code, autocorrect: false, decoration: InputDecoration(labelText: l10n.acctInviteCode)),
          TextField(key: const Key('truthid-username'), controller: _username, autocorrect: false, decoration: InputDecoration(labelText: l10n.acctUsername, hintText: 'ana.silva')),
          if (_error != null) Padding(padding: const EdgeInsets.only(top: 12), child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error))),
        ]),
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.of(context).pop(), child: Text(l10n.commonCancel)),
        FilledButton(onPressed: _busy ? null : _link, child: Text(l10n.acctLink)),
      ],
    );
  }
}
