// P137 — Warden's mark: a shield with a W, stroked in the purple gradient on near-black. The same drawing as `site/public/brand/icon.svg`
// (32x32 box), painted by hand because the app has no SVG package.

import 'package:flutter/material.dart';

class WardenLogo extends StatelessWidget {
  const WardenLogo({super.key, this.size = 48});

  final double size;

  @override
  Widget build(BuildContext context) => SizedBox.square(dimension: size, child: CustomPaint(painter: _LogoPainter()));
}

class _LogoPainter extends CustomPainter {
  static const _from = Color(0xFFC4B5FD);
  static const _to = Color(0xFF7C3AED);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.scale(size.width / 32, size.height / 32);
    final shader = const LinearGradient(begin: Alignment.topLeft, end: Alignment.bottomRight, colors: [_from, _to]).createShader(const Rect.fromLTWH(5, 2, 22, 28));
    final shield = Path()
      ..moveTo(16, 2.5)
      ..lineTo(27, 6.2)
      ..relativeLineTo(0, 8.6)
      ..relativeCubicTo(0, 6.4, -4.4, 11.1, -11, 14.2)
      ..cubicTo(9.4, 25.9, 5, 21.2, 5, 14.8)
      ..lineTo(5, 6.2)
      ..close();
    canvas.drawPath(shield, Paint()..color = const Color(0xFF0D0A18));
    canvas.drawPath(
      shield,
      Paint()
        ..shader = shader
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.8
        ..strokeJoin = StrokeJoin.round,
    );
    final w = Path()
      ..moveTo(10, 11.2)
      ..relativeLineTo(3.1, 9)
      ..relativeLineTo(2.9, -6.2)
      ..relativeLineTo(2.9, 6.2)
      ..relativeLineTo(3.1, -9);
    canvas.drawPath(
      w,
      Paint()
        ..shader = shader
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round,
    );
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
