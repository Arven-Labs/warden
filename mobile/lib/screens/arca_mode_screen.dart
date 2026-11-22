import 'package:flutter/material.dart';

import '../services/arca_mode_flow.dart';

/// The arca mode of this phone: turn it on, sync by hand, and see (and acknowledge) the newer key epochs that stop the vault.
///
/// STATE: tests first. This is an empty screen, so the tests fail for the right reason before the code exists.
class ArcaModeScreen extends StatefulWidget {
  const ArcaModeScreen({super.key, this.backend});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaModeBackend? backend;

  @override
  State<ArcaModeScreen> createState() => _ArcaModeScreenState();
}

class _ArcaModeScreenState extends State<ArcaModeScreen> {
  @override
  Widget build(BuildContext context) => const Scaffold(body: SizedBox.shrink());
}
