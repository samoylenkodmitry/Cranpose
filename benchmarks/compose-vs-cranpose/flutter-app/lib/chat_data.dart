// The chat beside the patchwork, as `perf-data/src/chat.rs` makes it bit for
// bit: an agent's answers arrive from a data thread as tokens, several a
// frame, and the screen shows them as they come. Under the list a status
// line says what the agent does.

import 'dart:math' as math;

const chatWidth = 320.0;
const chatTokensPerFrame = 4;
const chatTokenChars = 6;
const chatGifHold = 2;
const chatStatusHeight = 28.0;
const chatSpinnerSize = 16.0;
const chatSpinDegrees = 12.0;
const chatShimmerStep = 6.0;
const chatShimmerSpan = 120.0;
const chatGifSize = 64.0;
const chatText = 12.0;
const chatHeadingText = 14.0;
const chatCodeText = 11.0;
const chatBackgroundArgb = 0xFF14182B;
const chatUserArgb = 0xFF2B3A67;
const chatPanelArgb = 0xFF0B0E1A;
const chatShimmerDimArgb = 0xFF64748B;
const chatShimmerBrightArgb = 0xFFFFFFFF;

/// One block of a turn.
sealed class ChatBlock {
  const ChatBlock();
}

/// The user's question, whole at once in a bubble at the right.
class ChatUser extends ChatBlock {
  final String text;

  const ChatUser(this.text);
}

/// A heading in the bold face, which appears like a paragraph.
class ChatHeading extends ChatBlock {
  final String text;

  const ChatHeading(this.text);
}

/// Text that appears `chatTokenChars` characters a token.
class ChatParagraph extends ChatBlock {
  final String text;

  const ChatParagraph(this.text);
}

/// Code in a monospaced face with its stretches colored.
class ChatCode extends ChatBlock {
  final String text;

  const ChatCode(this.text);
}

/// A table whose first row is its header; a row appears a token.
class ChatTable extends ChatBlock {
  final List<List<String>> rows;

  const ChatTable(this.rows);
}

/// The animated picture.
class ChatGif extends ChatBlock {
  const ChatGif();
}

/// A row of buttons.
class ChatButtons extends ChatBlock {
  final List<String> labels;

  const ChatButtons(this.labels);
}

/// One turn of the chat.
const chatTurn = <ChatBlock>[
  ChatUser('Why does the list stutter when one row changes?'),
  ChatHeading('What happens in a frame'),
  ChatParagraph(
    'The list stutters because a small change starts a large amount of work. The row '
    'that changed shows a counter, and the counter gets a new value on every frame. '
    'The row measures its text again, which is right, because the text is new. The '
    'row then tells its parent that its size may have changed. The parent does not '
    'check whether the size really changed. It measures and places every one of its '
    'children again, the ones on the screen and the ones kept ready beyond its edges. '
    'Each child measures its own text again, although none of that text changed. A '
    'frame that should cost one row costs the whole list, and the cost grows with '
    'the number of rows, not with the size of the change.',
  ),
  ChatParagraph(
    'The profile shows the same thing from the other side. Most of the frame goes to '
    'text shaping, and nearly all of the shaped strings are equal to the strings of '
    'the frame before. The cache of shaped text does not help, because the list asks '
    'for each string at a width that differs by a fraction of a point from the width '
    'of the last frame, so every lookup misses. Drawing takes a small part of the '
    'frame and does not change, because the picture is almost the same.',
  ),
  ChatHeading('Three things to change'),
  ChatParagraph(
    '- Give the row a fixed height. A row whose size cannot change needs no new '
    'layout of its parent, so the work stops at the row.',
  ),
  ChatParagraph(
    '- Round the width that the list passes to its rows to whole points. Equal '
    'strings then meet equal widths, and the cache of shaped text answers.',
  ),
  ChatParagraph(
    '- Read the counter in the draw phase where the toolkit allows it. A value '
    'that changes only what is drawn should not start composition or layout.',
  ),
  ChatCode(
    '// Measure only the row that changed.\n'
    'fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n'
    '    let row = &mut rows[dirty];\n'
    '    if row.width == 320.0 {\n'
    '        return row.height;\n'
    '    }\n'
    '    row.label = "measured";\n'
    '    row.height = row.measure(320.0);\n'
    '    row.height\n'
    '}\n'
    '\n'
    '// A row of fixed height ends the pass.\n'
    'fn place(rows: &mut [Row], first: usize) {\n'
    '    let mut top = 0.0;\n'
    '    for row in &mut rows[first..] {\n'
    '        row.top = top;\n'
    '        top = top + row.height;\n'
    '    }\n'
    '}\n',
  ),
  ChatParagraph(
    'With the boundary in place the frame lays out one row and leaves the others '
    'alone. I ran the list of two thousand rows before and after the change, on the '
    'same device and at the same scroll position. These are the passes of one frame, '
    'as the median of three hundred frames:',
  ),
  ChatTable([
    ['Pass', 'Before', 'After', 'Change'],
    ['Compose', '1.92 ms', '0.31 ms', '-84%'],
    ['Measure', '4.70 ms', '0.22 ms', '-95%'],
    ['Place', '1.18 ms', '0.09 ms', '-92%'],
    ['Draw', '2.46 ms', '2.41 ms', '-2%'],
    ['Frame', '10.26 ms', '3.03 ms', '-70%'],
  ]),
  ChatGif(),
  ChatParagraph(
    'Drawing is now the largest pass, and it did not move. The next step is to keep '
    'the drawing of the rows that did not change and to draw only the row with the '
    'counter. That needs a look at how the list records its rows, which I have not '
    'read yet. The picture above turns while the benchmark runs; it stops when the '
    'run ends. Tell me whether to apply the three changes, or to show the diff first.',
  ),
  ChatButtons(['Apply', 'Show diff', 'Dismiss']),
  ChatParagraph('Waiting for your answer.'),
];

int _typedTokens(String text) => (text.length + chatTokenChars - 1) ~/ chatTokenChars;

/// The tokens [block] takes to appear whole.
int chatTokens(ChatBlock block) => switch (block) {
  ChatHeading(:final text) || ChatParagraph(:final text) || ChatCode(:final text) => _typedTokens(text),
  ChatTable(:final rows) => rows.length,
  _ => 1,
};

/// What the status line says the agent does while [block] is the last one.
String chatDoing(ChatBlock block) => switch (block) {
  ChatUser() => 'Reading',
  ChatHeading() || ChatParagraph() => 'Writing',
  ChatCode() => 'Writing code',
  ChatTable() => 'Filling the table',
  ChatGif() => 'Drawing',
  ChatButtons() => 'Waiting',
};

/// A token: its number from 1, the chat's last block and the tokens of that
/// block that arrived.
class ChatToken {
  final int count;
  final int block;
  final int shown;

  const ChatToken(this.count, this.block, this.shown);
}

final _starts = () {
  final starts = List.filled(chatTurn.length, 0);
  var total = 0;
  for (var index = 0; index < chatTurn.length; index++) {
    starts[index] = total;
    total += chatTokens(chatTurn[index]);
  }
  return starts;
}();
final _turnTokens = chatTurn.fold(0, (total, block) => total + chatTokens(block));

/// The chat after [count] tokens, [count] at least 1.
ChatToken chatTokenAfter(int count) {
  final index = math.max(count, 1) - 1;
  final turn = index ~/ _turnTokens;
  final within = index % _turnTokens;
  var block = chatTurn.length - 1;
  while (_starts[block] > within) {
    block--;
  }
  return ChatToken(count, turn * chatTurn.length + block, within - _starts[block] + 1);
}

/// The turn of block [block].
int chatTurnOf(int block) => block ~/ chatTurn.length;

/// The characters of [text] that [shown] tokens show.
int chatShownChars(String text, int shown) => math.min(shown * chatTokenChars, text.length);

/// The status line's text after token [token].
String chatStatus(ChatToken token) =>
    '${chatDoing(chatTurn[token.block % chatTurn.length])}… '
    '(${(token.count % 1000000).toString().padLeft(6, '0')} tokens)';

/// The mark of turn [turn] before a paragraph or a code block.
String chatTurnLabel(int turn) => '#${(turn % 100000).toString().padLeft(5, '0')}';

/// What a stretch of code is, for its color.
enum CodeKind {
  keyword(0xFFC084FC),
  text(0xFF86EFAC),
  number(0xFFFDBA74),
  comment(0xFF64748B);

  final int argb;

  const CodeKind(this.argb);
}

const _keywords = {'fn', 'let', 'mut', 'if', 'for', 'in', 'return', 'usize', 'f32', 'Row'};

/// A colored stretch of code: where it starts and ends, and its kind.
class CodeSpan {
  final int start;
  final int end;
  final CodeKind kind;

  const CodeSpan(this.start, this.end, this.kind);
}

bool _isLetter(int unit) => (unit >= 0x41 && unit <= 0x5A) || (unit >= 0x61 && unit <= 0x7A);

bool _isDigit(int unit) => unit >= 0x30 && unit <= 0x39;

/// The colored stretches of [code], in order. The code is ASCII.
List<CodeSpan> chatCodeSpans(String code) {
  final spans = <CodeSpan>[];
  var at = 0;
  while (at < code.length) {
    final start = at;
    final unit = code.codeUnitAt(at);
    if (code[at] == '/' && at + 1 < code.length && code[at + 1] == '/') {
      final end = code.indexOf('\n', at);
      at = end < 0 ? code.length : end;
      spans.add(CodeSpan(start, at, CodeKind.comment));
    } else if (code[at] == '"') {
      final end = code.indexOf('"', at + 1);
      at = end < 0 ? code.length : end + 1;
      spans.add(CodeSpan(start, at, CodeKind.text));
    } else if (_isLetter(unit) || code[at] == '_') {
      while (at < code.length && (_isLetter(code.codeUnitAt(at)) || _isDigit(code.codeUnitAt(at)) || code[at] == '_')) {
        at++;
      }
      if (_keywords.contains(code.substring(start, at))) spans.add(CodeSpan(start, at, CodeKind.keyword));
    } else if (_isDigit(unit)) {
      while (at < code.length && (_isDigit(code.codeUnitAt(at)) || code[at] == '.')) {
        at++;
      }
      spans.add(CodeSpan(start, at, CodeKind.number));
    } else {
      at++;
    }
  }
  return spans;
}

/// The frame of the animated picture that frame [frame] of the screen shows,
/// of [frames] frames.
int chatGifFrame(int frame, int frames) => (frame ~/ chatGifHold) % math.max(frames, 1);
