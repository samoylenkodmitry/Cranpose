// A value derived from the frame, which tells its listeners only when the
// value changes: a row of tiles that changes every 30th frame listens to its
// own step and not to the frame.

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

/// Builds below it once, with [select] of [frame] as a [ValueListenable]
/// that notifies when its value differs from the one before.
class FrameSelect<T> extends StatefulWidget {
  final ValueListenable<int> frame;
  final T Function(int frame) select;
  final Widget Function(BuildContext context, ValueListenable<T> value) builder;

  const FrameSelect(this.frame, this.select, this.builder, {super.key});

  @override
  State<FrameSelect<T>> createState() => _FrameSelectState<T>();
}

class _FrameSelectState<T> extends State<FrameSelect<T>> {
  late final _value = ValueNotifier<T>(widget.select(widget.frame.value));

  @override
  void initState() {
    super.initState();
    widget.frame.addListener(_update);
  }

  void _update() => _value.value = widget.select(widget.frame.value);

  @override
  void dispose() {
    widget.frame.removeListener(_update);
    _value.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _value);
}
