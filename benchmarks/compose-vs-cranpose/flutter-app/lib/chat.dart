// The chat beside the patchwork in Flutter (`../../README.md`): an agent's
// answers arrive from a data isolate as tokens and the list shows them as
// they come. Its data follows `perf-data/src/chat.rs` bit for bit.
//
// In Flutter's idiom the list is a lazy `ListView` that listens to the number
// of blocks only: a token rebuilds the text of the last block, which listens
// to how much of it shows, and no other block. Each block is a repaint
// boundary of the list's own, so a growing block paints again and the blocks
// above it move as layers. The list is reversed and shrink-wrapped: it rests
// at the top while short and at the bottom of its text once full, without a
// scroll offset to chase. The animated picture's frames are decoded once and
// shown by frame index; the spinner is a layer that turns; the status text
// takes the moving gradient as the shader of its paint.

import 'dart:convert';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'chat_data.dart';
import 'chat_feed.dart';
import 'frame_select.dart';
import 'host.dart';
import 'spinner.dart';
import 'wall.dart';

/// The family the monospaced face loads under.
const patchworkMono = 'PerfMono';

const _dim = Color(chatShimmerDimArgb);
const _bright = Color(chatShimmerBrightArgb);
const _panel = Color(chatPanelArgb);
const _shimmerColors = [_dim, _bright, _dim];
const _shimmerStops = [0.0, 0.5, 1.0];
const _bubble = Color(chatUserArgb);

/// The pictures of the chat: the frames of the animated picture and the
/// spinner's SVG.
class ChatAssets {
  final List<ui.Image> gif;
  final String svg;

  const ChatAssets(this.gif, this.svg);

  static Future<ChatAssets> load() async {
    final codec = await ui.instantiateImageCodec(await chatFile('spin.gif'));
    final gif = [for (var index = 0; index < codec.frameCount; index++) (await codec.getNextFrame()).image];
    return ChatAssets(gif, utf8.decode(await chatFile('progress.svg')));
  }
}

/// What the chat shows: its blocks so far, how much of the last one and the
/// last token, which [advance] writes as tokens arrive from the data isolate.
class ChatShown {
  final ChatFeed _feed;
  final blocks = ValueNotifier(0);
  final last = ValueNotifier(0);
  ChatToken? token;

  ChatShown(this._feed);

  /// Takes the frame's tokens. The screen never waits for a token.
  void advance() {
    var taken = 0;
    ChatToken? newest;
    while (taken < chatTokensPerFrame) {
      final arrived = _feed.take();
      if (arrived == null) break;
      taken++;
      newest = arrived;
    }
    if (newest == null) return;
    token = newest;
    blocks.value = newest.block + 1;
    last.value = newest.shown;
    _feed.grant(taken);
  }

  void dispose() {
    blocks.dispose();
    last.dispose();
    _feed.dispose();
  }
}

class Chat extends StatelessWidget {
  final ValueListenable<int> frame;
  final ChatShown shown;
  final ChatAssets assets;

  const Chat(this.frame, this.shown, this.assets, {super.key});

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: chatWidth,
      child: ColoredBox(
        color: const Color(chatBackgroundArgb),
        child: Column(
          children: [
            Expanded(child: RepaintBoundary(child: _Blocks(frame, shown, assets))),
            RepaintBoundary(child: _Status(frame, shown, assets.svg)),
          ],
        ),
      ),
    );
  }
}

/// The list of blocks: it listens to their number, so it builds again when a
/// block begins and not for the tokens of a block.
class _Blocks extends StatelessWidget {
  final ValueListenable<int> frame;
  final ChatShown shown;
  final ChatAssets assets;

  const _Blocks(this.frame, this.shown, this.assets);

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<int>(
      valueListenable: shown.blocks,
      builder: (context, count, _) => Align(
        alignment: Alignment.topCenter,
        child: ListView.builder(
          reverse: true,
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          padding: const EdgeInsets.fromLTRB(8, 8, 8, 0),
          itemCount: count,
          findChildIndexCallback: (key) => count - 1 - (key as ValueKey<int>).value,
          itemBuilder: (context, position) {
            final index = count - 1 - position;
            return Padding(
              key: ValueKey(index),
              padding: const EdgeInsets.only(bottom: 8),
              child: _Block(index, index + 1 == count, frame, shown, assets),
            );
          },
        ),
      ),
    );
  }
}

/// Block [index] of the chat; [live] while it is the last one, the one that
/// listens to how much of it shows.
class _Block extends StatelessWidget {
  final int index;
  final bool live;
  final ValueListenable<int> frame;
  final ChatShown shown;
  final ChatAssets assets;

  const _Block(this.index, this.live, this.frame, this.shown, this.assets);

  @override
  Widget build(BuildContext context) {
    final turn = chatTurnOf(index);
    final last = live ? shown.last : null;
    return switch (chatTurn[index % chatTurn.length]) {
      ChatUser(:final text) => Align(
        alignment: Alignment.centerRight,
        child: _Bubble(
          10,
          const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
          Text(text, style: inkText(chatText)),
        ),
      ),
      ChatHeading(:final text) => _Typed(turn, text, true, last),
      ChatParagraph(:final text) => _Typed(turn, text, false, last),
      ChatCode(:final text) => _Code(turn, text, last),
      ChatTable(:final rows) => _Table(rows, last),
      ChatGif() => _Gif(frame, assets.gif),
      ChatButtons(:final labels) => Row(
        spacing: 8,
        children: [
          for (final label in labels)
            _Bubble(
              6,
              const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              Text(label, style: inkText(chatText, bold: true)),
            ),
        ],
      ),
    };
  }
}

class _Bubble extends StatelessWidget {
  final double radius;
  final EdgeInsets padding;
  final Widget child;

  const _Bubble(this.radius, this.padding, this.child);

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: padding,
      decoration: BoxDecoration(color: _bubble, borderRadius: BorderRadius.circular(radius)),
      child: child,
    );
  }
}

/// A paragraph, or a heading in the bold face: the turn's number, then as
/// much of the text as arrived, while [last] says how much shows.
class _Typed extends StatelessWidget {
  final int turn;
  final String text;
  final bool heading;
  final ValueListenable<int>? last;

  const _Typed(this.turn, this.text, this.heading, this.last);

  @override
  Widget build(BuildContext context) {
    final style = inkText(heading ? chatHeadingText : chatText, bold: heading);
    final label = chatTurnLabel(turn);
    final last = this.last;
    if (last == null) return Text('$label $text', style: style);
    return ValueListenableBuilder<int>(
      valueListenable: last,
      builder: (context, shown, _) => Text('$label ${text.substring(0, chatShownChars(text, shown))}', style: style),
    );
  }
}

/// The whole block colored once: a comment that names the turn, then the code.
class _Code extends StatefulWidget {
  final int turn;
  final String code;
  final ValueListenable<int>? last;

  const _Code(this.turn, this.code, this.last);

  @override
  State<_Code> createState() => _CodeState();
}

class _CodeState extends State<_Code> {
  late final int _head = '// ${chatTurnLabel(widget.turn)}\n'.length;
  late final List<(String, Color?)> _segments = _highlight();

  List<(String, Color?)> _highlight() {
    final code = widget.code;
    final segments = <(String, Color?)>[('// ${chatTurnLabel(widget.turn)}\n', Color(CodeKind.comment.argb))];
    var at = 0;
    for (final span in chatCodeSpans(code)) {
      segments.add((code.substring(at, span.start), null));
      segments.add((code.substring(span.start, span.end), Color(span.kind.argb)));
      at = span.end;
    }
    segments.add((code.substring(at), null));
    return segments;
  }

  /// The first [chars] characters of the colored text.
  TextSpan _upTo(int chars) {
    final spans = <TextSpan>[];
    var left = chars;
    for (final (text, color) in _segments) {
      if (left <= 0) break;
      final cut = math.min(left, text.length);
      spans.add(
        TextSpan(
          text: text.substring(0, cut),
          style: TextStyle(color: color),
        ),
      );
      left -= cut;
    }
    return TextSpan(children: spans);
  }

  @override
  Widget build(BuildContext context) {
    final style = inkText(chatCodeText).copyWith(fontFamily: patchworkMono);
    final last = widget.last;
    return Container(
      padding: const EdgeInsets.all(8),
      decoration: BoxDecoration(color: _panel, borderRadius: BorderRadius.circular(6)),
      child: last == null
          ? Text.rich(_upTo(_head + widget.code.length), style: style)
          : ValueListenableBuilder<int>(
              valueListenable: last,
              builder: (context, shown, _) =>
                  Text.rich(_upTo(_head + chatShownChars(widget.code, shown)), style: style),
            ),
    );
  }
}

/// The table: its rows so far, a row a token while [last] says how many show.
class _Table extends StatelessWidget {
  final List<List<String>> rows;
  final ValueListenable<int>? last;

  const _Table(this.rows, this.last);

  @override
  Widget build(BuildContext context) {
    final last = this.last;
    if (last == null) return _rowsUpTo(rows.length);
    return ValueListenableBuilder<int>(
      valueListenable: last,
      builder: (context, shown, _) => _rowsUpTo(math.min(shown, rows.length)),
    );
  }

  Widget _rowsUpTo(int count) {
    return Column(
      children: [
        for (var index = 0; index < count; index++)
          Container(
            color: index == 0 || index % 2 == 1 ? _panel : null,
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
            child: Row(
              children: [
                for (final cell in rows[index])
                  Expanded(
                    child: Text(cell, style: inkText(chatCodeText, bold: index == 0)),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

/// The animated picture: the one widget of its block that listens to the
/// frame, through the index of the picture's frame.
class _Gif extends StatelessWidget {
  final ValueListenable<int> frame;
  final List<ui.Image> frames;

  const _Gif(this.frame, this.frames);

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.centerLeft,
      child: SizedBox(
        width: chatGifSize,
        height: chatGifSize,
        child: FrameSelect<int>(
          frame,
          (frame) => chatGifFrame(frame, frames.length),
          (context, index) => ValueListenableBuilder<int>(
            valueListenable: index,
            builder: (context, index, _) => RawImage(image: frames[index], fit: BoxFit.fill),
          ),
        ),
      ),
    );
  }
}

/// The status line under the list: a spinner that turns each frame and what
/// the agent does, under a gradient that moves each frame.
class _Status extends StatelessWidget {
  final ValueListenable<int> frame;
  final ChatShown shown;
  final String svg;

  const _Status(this.frame, this.shown, this.svg);

  @override
  Widget build(BuildContext context) {
    return Container(
      height: chatStatusHeight,
      color: _panel,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        spacing: 6,
        children: [
          ValueListenableBuilder<int>(
            valueListenable: frame,
            child: Spinner(svg, chatSpinnerSize),
            builder: (context, frame, spinner) =>
                Transform.rotate(angle: frame * chatSpinDegrees * math.pi / 180, child: spinner),
          ),
          ValueListenableBuilder<int>(
            valueListenable: frame,
            builder: (context, frame, _) {
              final token = shown.token;
              if (token == null) return const SizedBox.shrink();
              final start = (frame * chatShimmerStep) % chatShimmerSpan;
              final shimmer = Paint()
                ..shader = ui.Gradient.linear(
                  Offset(start, 0),
                  Offset(start + chatShimmerSpan, 0),
                  _shimmerColors,
                  _shimmerStops,
                  TileMode.repeated,
                );
              return Text(
                chatStatus(token),
                style: TextStyle(fontSize: chatText, foreground: shimmer),
              );
            },
          ),
        ],
      ),
    );
  }
}
