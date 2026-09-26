package com.compose_rs.demo;

import android.graphics.Rect;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;

import androidx.test.platform.app.InstrumentationRegistry;

import dev.cranpose.android.CranposeActivity;

import org.junit.Test;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Constructor;
import java.util.List;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

public final class CranposeAccessibilityParserTest {
    private static List<?> parse(String payload) throws Exception {
        Method method = CranposeActivity.class.getDeclaredMethod(
                "parseAccessibilityElements", String.class);
        method.setAccessible(true);
        return (List<?>) method.invoke(null, payload);
    }

    private static Object field(Object element, String name) throws Exception {
        Field field = element.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return field.get(element);
    }

    private static String record(String id, String label, String actions) {
        return String.join("\t", id, "5", "2", "4", "62", "84", "16", "22",
                "1", label, "", "On", "Toggle", "-1", "1", "1", actions,
                "1", "0", "0", "0", "0", "0", "0", "0", "0", "-1", "0", "0", "0", "-1", "-1", "", "", "0", "-1",
                "Remove receipt", "0", "0", "-1", "-1");
    }

    @Test
    public void preservesTrailingEmptyFieldsAndEscapedDelimiters() throws Exception {
        String escaped = "A%2509%09B%0AC%0D%1F\uD83C\uDF17";
        List<?> elements = parse(record("17", escaped, "Pause%1Finside\u001f")
                + "\n" + record("18", "Empty actions", "") + "\n\n");
        assertEquals(2, elements.size());
        Object first = elements.get(0);
        assertEquals(17, field(first, "id"));
        assertEquals(5, field(first, "role"));
        assertEquals(new Rect(2, 4, 62, 84), field(first, "bounds"));
        assertEquals(16.0f, field(first, "centerX"));
        assertEquals(22.0f, field(first, "centerY"));
        assertEquals(true, field(first, "clickable"));
        assertEquals("A%09\tB\nC\r\u001f\uD83C\uDF17", field(first, "label"));
        assertEquals("", field(first, "value"));
        assertEquals("On", field(first, "stateDescription"));
        assertEquals("Toggle", field(first, "clickLabel"));
        assertEquals("Remove receipt", field(first, "longClickLabel"));
        assertEquals(-1, field(first, "selected"));
        assertEquals(1, field(first, "toggled"));
        assertEquals(true, field(first, "enabled"));
        assertArrayEquals(new String[]{"Pause\u001finside", ""},
                (String[]) field(first, "customActions"));
        assertEquals(18, field(elements.get(1), "id"));
        assertArrayEquals(new String[0], (String[]) field(elements.get(1), "customActions"));
    }

    @Test
    public void skipsMalformedRecordsWithoutDiscardingFollowingNodes() throws Exception {
        List<?> elements = parse("short\trow\n" + record("bad id", "Invalid", "")
                + "\n" + record("19", "Too many", "") + "\textra"
                + "\n" + record("20", "Valid", ""));
        assertEquals(1, elements.size());
        assertEquals(20, field(elements.get(0), "id"));
        assertEquals("Valid", field(elements.get(0), "label"));
    }

    @Test
    public void emptyPayloadsHaveNoNodes() throws Exception {
        assertTrue(parse(null).isEmpty());
        assertTrue(parse("").isEmpty());
        assertTrue(parse("\n\n").isEmpty());
    }

    private static AccessibilityNodeProvider provider(String payload) throws Exception {
        Class<?> type = Class.forName(CranposeActivity.class.getName() + "$CranposeAccessibilityProvider");
        Constructor<?> constructor = type.getDeclaredConstructor(View.class);
        constructor.setAccessible(true);
        Object provider = constructor.newInstance(new View(
                InstrumentationRegistry.getInstrumentation().getTargetContext()));
        Field elements = type.getDeclaredField("elements");
        elements.setAccessible(true);
        elements.set(provider, parse(payload));
        return (AccessibilityNodeProvider) provider;
    }

    @Test
    public void progressIndicatorsReportTheirRangeWithoutOfferingAdjustment() throws Exception {
        String[] fields = record("21", "Loading", "").split("\t", -1);
        fields[1] = "15";
        fields[20] = "40";
        fields[21] = "0";
        fields[22] = "100";
        AccessibilityNodeInfo node = provider(String.join("\t", fields)).createAccessibilityNodeInfo(21);
        assertEquals("android.widget.ProgressBar", node.getClassName());
        assertNotNull(node.getRangeInfo());
        assertEquals(40.0f, node.getRangeInfo().getCurrent(), 0.0f);
        assertEquals(100.0f, node.getRangeInfo().getMax(), 0.0f);
        assertFalse(node.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS));
    }

    @Test
    public void disabledControlsRejectActionsButRemainDiscoverable() throws Exception {
        String[] fields = record("22", "Disabled", "Remove").split("\t", -1);
        fields[15] = "0";
        AccessibilityNodeProvider provider = provider(String.join("\t", fields));
        AccessibilityNodeInfo node = provider.createAccessibilityNodeInfo(22);
        assertFalse(node.isEnabled());
        assertTrue(node.isVisibleToUser());
        assertFalse(provider.performAction(22, AccessibilityNodeInfo.ACTION_CLICK, null));
        assertFalse(provider.performAction(22, AccessibilityNodeInfo.ACTION_LONG_CLICK, null));
        assertFalse(provider.performAction(22, AccessibilityNodeInfo.ACTION_SET_TEXT, null));
        assertFalse(provider.performAction(22, AccessibilityNodeInfo.ACTION_DISMISS, null));
        assertEquals(2, node.getActionList().size());
        assertFalse(node.isClickable());
        assertFalse(node.isLongClickable());
        assertTrue(node.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_SHOW_ON_SCREEN));
        assertTrue(node.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_ACCESSIBILITY_FOCUS));
    }

    @Test
    public void textSearchMatchesNamesAndValuesWithinTheRequestedSubtree() throws Exception {
        String[] child = record("32", "Title", "").split("\t", -1);
        child[1] = "3";
        child[10] = "Café notes";
        child[26] = "31";
        AccessibilityNodeProvider provider = provider(record("31", "Notebook", "")
                + "\n" + String.join("\t", child) + "\n" + record("33", "Other notes", ""));
        assertEquals(2, provider.findAccessibilityNodeInfosByText("NOTES", -1).size());
        assertEquals(1, provider.findAccessibilityNodeInfosByText("notes", 31).size());
        assertEquals(1, provider.findAccessibilityNodeInfosByText("CAFÉ", 32).size());
        assertEquals("Notebook", provider.findAccessibilityNodeInfosByText("book", 31)
                .get(0).getContentDescription());
        assertTrue(provider.findAccessibilityNodeInfosByText("notes", 99).isEmpty());
        assertTrue(provider.findAccessibilityNodeInfosByText("absent", -1).isEmpty());
    }

    private static boolean update(AccessibilityNodeProvider provider, int[] order, String records,
            int[] moves) throws Exception {
        Method method = provider.getClass().getDeclaredMethod(
                "update", int[].class, List.class, int[].class);
        method.setAccessible(true);
        return (Boolean) method.invoke(provider, order, parse(records), moves);
    }

    private static Rect bounds(AccessibilityNodeProvider provider, int id) {
        Rect rect = new Rect();
        provider.createAccessibilityNodeInfo(id).getBoundsInParent(rect);
        return rect;
    }

    @Test
    public void updatesKeepUnsentControlsResendChangedOnesAndMoveBounds() throws Exception {
        AccessibilityNodeProvider provider = provider("");
        assertTrue(update(provider, new int[]{41, 42, 43},
                record("41", "One", "") + "\n" + record("42", "Two", "") + "\n"
                        + record("43", "Three", ""), new int[0]));

        assertTrue(update(provider, new int[]{43, 41}, record("43", "Renamed", ""),
                new int[]{41, 5, 6, 7, 8}));

        assertEquals("Renamed", provider.createAccessibilityNodeInfo(43).getContentDescription());
        assertEquals("One", provider.createAccessibilityNodeInfo(41).getContentDescription());
        assertEquals(new Rect(5, 6, 7, 8), bounds(provider, 41));
        assertEquals(new Rect(2, 4, 62, 84), bounds(provider, 43));
        assertEquals(null, provider.createAccessibilityNodeInfo(42));
    }

    @Test
    public void anUpdateNamingAnUnknownControlAsksForEveryRecord() throws Exception {
        AccessibilityNodeProvider provider = provider(record("51", "Kept", ""));
        assertFalse(update(provider, new int[]{51, 52}, "", new int[0]));
        assertEquals("Kept", provider.createAccessibilityNodeInfo(51).getContentDescription());
    }
}
