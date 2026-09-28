package dev.cranpose.twin

import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.io.File

/**
 * The scenes, by the name both frameworks give their screenshot. Each one is
 * a desktop-demo composable (apps/desktop-demo/src/app.rs) written in Compose
 * with the same structure, modifier chains in the same order and the same
 * constants. Cranpose's `background(c).rounded_corners(r)` is Compose's
 * `background(c, RoundedCornerShape(r))`.
 */
/** A frame both frameworks draw: its name, its density, what it shows. */
class TwinFrame(val name: String, val density: Float, val content: @Composable () -> Unit)

val SCENES: List<Pair<String, @Composable () -> Unit>> = listOf(
    "simple-card" to { SimpleCardShowcase() },
    "positioned-boxes" to { PositionedBoxesShowcase() },
    "item-list" to { ItemListShowcase() },
    "complex-chain" to { ComplexChainShowcase() },
    "modifier-order" to { ModifierOrderProbes() },
    "text-fields" to { TextFieldProbes() },
    "text-selection" to { TextSelectionProbe() },
    "scroll-minimum" to { ScrollMinimumProbe() },
)

/** The demo's fonts, loaded from the files Cranpose embeds. */
object DemoFonts {
    lateinit var family: FontFamily

    fun load(regular: File, bold: File) {
        family = FontFamily(Font(regular), Font(bold, FontWeight.Bold))
    }
}

/** Cranpose's `TextStyle::default()`: white, 14 sp, the demo's font. */
fun defaultTextStyle() = TextStyle(color = Color.White, fontSize = 14.sp, fontFamily = DemoFonts.family)

@Composable
fun Text(text: String, modifier: Modifier) {
    BasicText(text, modifier, style = defaultTextStyle())
}

/** The canvas both frameworks draw a scene on, content at the top left. */
@Composable
fun SceneFrame(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().background(Color(0.07f, 0.07f, 0.09f, 1f))) {
        content()
    }
}

@Composable
fun SimpleCardShowcase() {
    Column(Modifier) {
        Text(
            "=== Simple Card Pattern ===",
            Modifier.background(Color(1f, 1f, 1f, 0.1f), RoundedCornerShape(14.dp)).padding(12.dp),
        )
        Spacer(Modifier.size(0.dp, 16.dp))
        Box(Modifier.background(Color(0.4f, 0.6f, 0.9f, 0.8f), RoundedCornerShape(18.dp)).padding(3.dp)) {
            Box(
                Modifier.background(Color(0.15f, 0.18f, 0.25f, 0.95f), RoundedCornerShape(16.dp))
                    .padding(16.dp).size(300.dp, 200.dp),
            ) {
                Column(Modifier.padding(8.dp)) {
                    Text(
                        "Card Title",
                        Modifier.background(Color(0.3f, 0.5f, 0.8f, 0.6f), RoundedCornerShape(8.dp)).padding(8.dp),
                    )
                    Spacer(Modifier.size(0.dp, 8.dp))
                    Text("Card content goes here with padding", Modifier.padding(4.dp))
                    Spacer(Modifier.size(0.dp, 12.dp))
                    Row(Modifier) {
                        Text(
                            "Action 1",
                            Modifier.background(Color(0.2f, 0.7f, 0.4f, 0.7f), RoundedCornerShape(6.dp)).padding(8.dp),
                        )
                        Spacer(Modifier.size(8.dp, 0.dp))
                        Text(
                            "Action 2",
                            Modifier.background(Color(0.8f, 0.3f, 0.3f, 0.7f), RoundedCornerShape(6.dp)).padding(8.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun PositionedBoxesShowcase() {
    Column(Modifier) {
        Text(
            "=== Positioned Boxes ===",
            Modifier.background(Color(1f, 1f, 1f, 0.1f), RoundedCornerShape(14.dp)).padding(12.dp),
        )
        Spacer(Modifier.size(0.dp, 16.dp))
        Box(Modifier.size(360.dp, 280.dp).background(Color(0.05f, 0.05f, 0.15f, 0.5f), RoundedCornerShape(8.dp))) {
            PositionedBox("Box A", 100, 100, 20, 20, 8, Color(0.6f, 0.2f, 0.7f, 0.85f), 12, 6)
            PositionedBox("Box B", 100, 100, 220, 160, 8, Color(0.2f, 0.7f, 0.4f, 0.85f), 12, 6)
            PositionedBox("C", 80, 60, 140, 30, 6, Color(0.9f, 0.5f, 0.2f, 0.85f), 10, 4)
            PositionedBox("Box D", 120, 80, 40, 140, 8, Color(0.2f, 0.5f, 0.9f, 0.85f), 14, 6)
        }
    }
}

/** One of `positioned_boxes_showcase`'s boxes: size, offset, padding, a
 * rounded background and a padded label. */
@Composable
private fun PositionedBox(
    label: String,
    width: Int,
    height: Int,
    x: Int,
    y: Int,
    padding: Int,
    color: Color,
    radius: Int,
    labelPadding: Int,
) {
    Box(
        Modifier.size(width.dp, height.dp).offset(x.dp, y.dp)
            .background(color, RoundedCornerShape(radius.dp)).padding(padding.dp),
    ) {
        Text(label, Modifier.padding(labelPadding.dp))
    }
}

@Composable
fun ItemListShowcase() {
    Column(Modifier) {
        Text(
            "=== Item List (5 items) ===",
            Modifier.background(Color(1f, 1f, 1f, 0.1f), RoundedCornerShape(14.dp)).padding(12.dp),
        )
        Spacer(Modifier.size(0.dp, 16.dp))
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for (i in 0 until 5) {
                val (bg, border) = if (i % 2 == 0) {
                    Color(0.12f, 0.16f, 0.28f, 0.8f) to Color(0.3f, 0.5f, 0.8f, 0.9f)
                } else {
                    Color(0.18f, 0.12f, 0.28f, 0.8f) to Color(0.5f, 0.3f, 0.8f, 0.9f)
                }
                Box(Modifier.background(border, RoundedCornerShape(12.dp)).padding(2.dp)) {
                    Row(Modifier.background(bg, RoundedCornerShape(10.dp)).padding(8.dp).size(400.dp, 50.dp)) {
                        Text("Item #$i", Modifier.padding(horizontal = 12.dp))
                        Spacer(Modifier.size(0.dp, 0.dp))
                        val status = when (i % 3) {
                            0 -> Color(0.2f, 0.8f, 0.3f, 0.9f)
                            1 -> Color(0.9f, 0.7f, 0.2f, 0.9f)
                            else -> Color(0.8f, 0.3f, 0.2f, 0.9f)
                        }
                        Box(Modifier.size(12.dp, 12.dp).background(status, RoundedCornerShape(6.dp)))
                    }
                }
            }
        }
    }
}

@Composable
fun ComplexChainShowcase() {
    Column(Modifier) {
        Text(
            "=== Complex Modifier Chain ===",
            Modifier.background(Color(1f, 1f, 1f, 0.1f), RoundedCornerShape(14.dp)).padding(12.dp),
        )
        Spacer(Modifier.size(0.dp, 16.dp))
        Text("Nested: Red → Green → Blue layers", Modifier.padding(8.dp))
        Spacer(Modifier.size(0.dp, 12.dp))
        Box(Modifier.background(Color(0.8f, 0.2f, 0.2f, 0.9f), RoundedCornerShape(16.dp)).padding(8.dp)) {
            Box(Modifier.background(Color(0.2f, 0.7f, 0.3f, 0.9f), RoundedCornerShape(12.dp)).padding(6.dp)) {
                Box(Modifier.background(Color(0.3f, 0.5f, 0.9f, 0.9f), RoundedCornerShape(8.dp)).padding(12.dp)) {
                    Text("Nested!", Modifier)
                }
            }
        }
        Spacer(Modifier.size(0.dp, 16.dp))
        Text("Chain: offset + size + multiple backgrounds", Modifier.padding(8.dp))
        Spacer(Modifier.size(0.dp, 12.dp))
        Box(
            Modifier.offset(20.dp, 0.dp).size(180.dp, 80.dp)
                .background(Color(0.9f, 0.6f, 0.2f, 0.9f), RoundedCornerShape(10.dp)).padding(6.dp),
        ) {
            Box(Modifier.background(Color(0.5f, 0.3f, 0.7f, 0.9f), RoundedCornerShape(6.dp)).padding(8.dp)) {
                Text("Offset + Sized", Modifier)
            }
        }
    }
}

/** `modifier_order_probes` in `test_screens/compose_twin.rs`. */
@Composable
fun ModifierOrderProbes() {
    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(
            Modifier.fillMaxWidth().padding(4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("API Endpoint:", Modifier.padding(2.dp))
            Text("https://api.ipify.org", Modifier.padding(2.dp).minimumInteractiveComponentSize())
        }
        Box(Modifier.background(Color(0.8f, 0.3f, 0.3f, 0.9f)).offset(12.dp, 6.dp).size(80.dp, 40.dp))
        Box(Modifier.offset(12.dp, 6.dp).background(Color(0.3f, 0.6f, 0.9f, 0.9f)).size(80.dp, 40.dp))
        Box(Modifier.padding(6.dp).minimumInteractiveComponentSize().background(Color(0.3f, 0.8f, 0.4f, 0.9f))) {
            Box(Modifier.size(16.dp, 16.dp))
        }
    }
}

/** The fill behind each probe field. */
val FieldFill = Color(0.25f, 0.27f, 0.33f, 1f)

@Composable
fun TextFieldProbes() {
    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        BasicTextField("", {}, Modifier.background(FieldFill), textStyle = defaultTextStyle())
        BasicTextField("Hi", {}, Modifier.background(FieldFill).padding(4.dp), textStyle = defaultTextStyle())
        BasicTextField(
            "A field as wide as its text",
            {},
            Modifier.background(FieldFill),
            textStyle = defaultTextStyle(),
        )
        BasicTextField(
            "",
            {},
            Modifier.background(Color(0.2f, 0.35f, 0.6f, 1f)).padding(9.dp),
            textStyle = defaultTextStyle(),
            decorationBox = { inner ->
                Box {
                    BasicText("Search", style = defaultTextStyle().copy(color = Color(1f, 1f, 1f, 0.5f)))
                    inner()
                }
            },
        )
    }
}

/** The accent a focused field tints its caret and selection with. */
val FieldAccent = Color(0.30f, 0.55f, 0.90f, 1f)

@Composable
fun TextSelectionProbe() {
    val requester = remember { FocusRequester() }
    CompositionLocalProvider(
        LocalTextSelectionColors provides TextSelectionColors(
            handleColor = FieldAccent,
            backgroundColor = FieldAccent.copy(alpha = 0.32f),
        ),
    ) {
        Column(Modifier.padding(16.dp)) {
            BasicTextField(
                TextFieldValue("Select some text", selection = TextRange(0, 6)),
                {},
                Modifier.focusRequester(requester)
                    .background(FieldFill)
                    .padding(start = 10.dp, top = 4.dp, end = 6.dp, bottom = 8.dp),
                textStyle = defaultTextStyle(),
                cursorBrush = SolidColor(FieldAccent),
            )
        }
    }
    LaunchedEffect(Unit) { requester.requestFocus() }
}

@Composable
fun ScrollMinimumProbe() {
    Column(
        Modifier.fillMaxSize()
            .verticalScroll(rememberScrollState())
            .background(Color(0.12f, 0.16f, 0.28f, 1f)),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.size(120.dp, 60.dp).background(Color(0.9f, 0.5f, 0.2f, 1f), RoundedCornerShape(8.dp)))
    }
}
