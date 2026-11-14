import 'package:flutter/material.dart';

import '../services/arca_pair_flow.dart';

/// Pairs this phone, as the NEW device, with another that already has the vault key (arca pairing through the hub).
class ArcaPairScreen extends StatefulWidget {
  const ArcaPairScreen({super.key, this.backend});

  /// Tests pass a fake; the app uses the Rust bridge.
  final ArcaPairBackend? backend;

  @override
  State<ArcaPairScreen> createState() => _ArcaPairScreenState();
}

class _ArcaPairScreenState extends State<ArcaPairScreen> {
  @override
  Widget build(BuildContext context) {
    // Stub: the tests are the contract; the screen comes after they are reviewed.
    return const Scaffold();
  }
}
