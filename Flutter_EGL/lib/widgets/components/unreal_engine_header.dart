import 'package:flutter/material.dart';

class UnrealEngineHeader extends StatelessWidget {
  final String text;
  final TextAlign? textAlign;
  final TextStyle? style;

  const UnrealEngineHeader(
      this.text, {
        super.key,
        this.textAlign,
        this.style,
      });

  @override
  Widget build(BuildContext context) {
    final defaultStyle = Theme.of(context).textTheme.titleLarge?.copyWith(
      fontWeight: FontWeight.w800,
      color: Theme.of(context).colorScheme.onSurface,
    );

    final effectiveStyle =
    style == null ? defaultStyle : defaultStyle?.merge(style) ?? style;

    return Text(
      text,
      textAlign: textAlign,
      style: effectiveStyle,
    );
  }
}
