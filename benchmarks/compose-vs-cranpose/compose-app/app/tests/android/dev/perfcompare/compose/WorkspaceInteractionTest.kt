package dev.perfcompare.compose

import android.view.View
import androidx.compose.ui.focus.FocusManager
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.test.captureToImage
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.platform.app.InstrumentationRegistry
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsProperties
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Rule
import org.junit.Test
import org.junit.Before

class WorkspaceInteractionTest {
    @get:Rule
    val compose = createComposeRule()

    @Before
    fun keepAnimationFramesUnderTestControl() {
        compose.mainClock.autoAdvance = false
    }

    @Test
    fun dockTabsOpenTheirSubscribedPanelsAndReturnToQuotes() {
        compose.setContent { WorkspaceFrame(WorkspaceMode.Quotes) }
        compose.onNodeWithText("Profile").performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Profile panel").assertExists()
        compose.onNodeWithText("Quote").performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Profile panel").assertDoesNotExist()
        compose.onNodeWithContentDescription("Selected quote price").assertExists()
    }

    @Test
    fun searchAcceptsTypedSymbols() {
        compose.setContent { WorkspaceFrame(WorkspaceMode.Quotes) }
        compose.onNodeWithContentDescription("Search symbols").performTextInput("AAPL")
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithText("AAPL").assertExists()
    }

    @Test
    fun searchFocusKeepsTheKeyboardHiddenUntilTapped() {
        lateinit var view: View
        lateinit var focus: FocusManager
        compose.setContent {
            view = LocalView.current
            focus = LocalFocusManager.current
            WorkspaceFrame(WorkspaceMode.Quotes, still = true)
        }
        val search = compose.onNodeWithContentDescription("Search symbols")
        val originalBounds = search.fetchSemanticsNode().boundsInRoot
        search.performTouchInput { click() }
        compose.mainClock.advanceTimeByFrame()
        waitForKeyboard(view, visible = true)
        compose.runOnIdle { focus.clearFocus(force = true) }
        compose.mainClock.advanceTimeByFrame()
        waitForKeyboard(view, visible = false)
        compose.mainClock.advanceTimeBy(500)

        search.performSemanticsAction(SemanticsActions.RequestFocus) { it() }
        compose.mainClock.advanceTimeByFrame()
        search.assertIsFocused()
        InstrumentationRegistry.getInstrumentation().uiAutomation.waitForIdle(500, 5_000)
        assertFalse("Programmatic search focus must not open the keyboard", keyboardVisible(view))
        assertEquals("The tap target must settle after the IME hides", originalBounds, search.fetchSemanticsNode().boundsInRoot)

        search.performTouchInput { click() }
        compose.mainClock.advanceTimeByFrame()
        waitForKeyboard(view, visible = true)
        compose.runOnIdle { focus.clearFocus(force = true) }
    }

    private fun keyboardVisible(view: View): Boolean = compose.runOnIdle {
        ViewCompat.getRootWindowInsets(view)?.isVisible(WindowInsetsCompat.Type.ime()) == true
    }

    private fun waitForKeyboard(view: View, visible: Boolean) {
        compose.waitUntil(5_000) {
            compose.mainClock.advanceTimeByFrame()
            keyboardVisible(view) == visible
        }
    }

    @Test
    fun quoteStreamingCanBeStoppedAndStarted() {
        compose.setContent { WorkspaceFrame(WorkspaceMode.Quotes) }
        compose.onNodeWithContentDescription("Stream quotes").assertIsOn().performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Stream quotes").assertIsOff()
        val quote = compose.onNodeWithContentDescription("Selected quote price")
        val paused = quote.fetchSemanticsNode().config[SemanticsProperties.Text]
        compose.mainClock.advanceTimeBy(128)
        assertEquals(paused, quote.fetchSemanticsNode().config[SemanticsProperties.Text])
        compose.onNodeWithContentDescription("Stream quotes").performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Stream quotes").assertIsOn()
        compose.mainClock.advanceTimeBy(128)
        assertNotEquals(paused, quote.fetchSemanticsNode().config[SemanticsProperties.Text])
    }

    @Test
    fun watchlistAutoScrollShowsItsSelectedModeAndStops() {
        compose.setContent { WorkspaceFrame(WorkspaceMode.Quotes) }
        val row = compose.onNodeWithContentDescription("Watchlist row 10")
        val initial = row.fetchSemanticsNode().boundsInRoot.top
        compose.onNodeWithContentDescription("Auto-scroll Watchlist").performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Auto-scroll Watchlist").assertIsSelected()
        compose.mainClock.advanceTimeBy(128)
        assertNotEquals(initial, row.fetchSemanticsNode().boundsInRoot.top)
        compose.onNodeWithContentDescription("Auto-scroll Off").performClick()
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithContentDescription("Auto-scroll Off").assertIsSelected()
        val stopped = row.fetchSemanticsNode().boundsInRoot.top
        compose.mainClock.advanceTimeBy(128)
        assertEquals(stopped, row.fetchSemanticsNode().boundsInRoot.top)
        compose.onNodeWithText("Introduction").assertExists()
    }

    @Test
    fun hoverModeHighlightsTheRowUnderItsPointer() {
        compose.setContent { WorkspaceFrame(WorkspaceMode.Hover) }
        val row = compose.onNodeWithContentDescription("Watchlist row 0")
        val before = row.captureToImage().toPixelMap()
        compose.mainClock.advanceTimeByFrame()
        compose.mainClock.advanceTimeByFrame()
        val after = row.captureToImage().toPixelMap()
        assertNotEquals(before[3, before.height / 2], after[3, after.height / 2])
    }
}
