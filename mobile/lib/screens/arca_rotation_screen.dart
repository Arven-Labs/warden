import 'package:flutter/material.dart';

import '../services/arca_mode_flow.dart';

/// The key rotation for "I lost a device" (P-49): see the plan in `project/PENDING.md`. Pops `true` once the rotation was confirmed.
class ArcaRotationScreen extends StatefulWidget {
  const ArcaRotationScreen({super.key, required this.backend});

  final ArcaModeBackend backend;

  @override
  State<ArcaRotationScreen> createState() => _ArcaRotationScreenState();
}

class _ArcaRotationScreenState extends State<ArcaRotationScreen> {
  @override
  Widget build(BuildContext context) => const Scaffold(body: SizedBox());
}
