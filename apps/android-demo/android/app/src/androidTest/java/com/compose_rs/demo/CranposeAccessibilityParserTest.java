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
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.List;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

public final class CranposeAccessibilityParserTest {
    private static Object field(Object element, String name) throws Exception {
        Field field = element.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return field.get(element);
    }

    /**
     * One control's record as android_accessibility_wire.rs writes it, its
     * fields in wire order and set by index: N a little-endian int, F a
     * float, T UTF-8 text after its length, A a count of labels, then each.
     */
    private static final class Record {
        private static final String KINDS = "NNNNNNFFNTTTTNNNANNNFFFNNNNNNNNNTTNNTNNNN";
        final Object[] fields;

        Record(int id, String label, String... actions) {
            fields = new Object[]{id, 5, 2, 4, 62, 84, 16f, 22f, 1, label, "", "On", "Toggle", -1,
                    1, 1, actions, 1, 0, 0, 0f, 0f, 0f, 0, 0, 0, -1, 0, 0, 0, -1, -1, "", "", 0,
                    -1, "Remove receipt", 0, 0, -1, -1};
        }

        Record set(int index, Object value) {
            fields[index] = value;
            return this;
        }

        void writeTo(ByteBuffer out) {
            for (int i = 0; i < fields.length; i++) {
                switch (KINDS.charAt(i)) {
                    case 'N': out.putInt((Integer) fields[i]); break;
                    case 'F': out.putFloat((Float) fields[i]); break;
                    case 'T': text(out, (String) fields[i]); break;
                    default:
                        String[] labels = (String[]) fields[i];
                        out.putInt(labels.length);
                        for (String label : labels) text(out, label);
                }
            }
        }

        private static void text(ByteBuffer out, String value) {
            byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
            out.putInt(bytes.length);
            out.put(bytes);
        }
    }

    private static Record record(int id, String label, String... actions) {
        return new Record(id, label, actions);
    }

    private static byte[] payload(Record... records) {
        ByteBuffer out = ByteBuffer.allocate(1 << 16).order(ByteOrder.LITTLE_ENDIAN);
        for (Record record : records) record.writeTo(out);
        return Arrays.copyOf(out.array(), out.position());
    }

    private static int[] ids(Record... records) {
        int[] ids = new int[records.length];
        for (int i = 0; i < records.length; i++) ids[i] = (Integer) records[i].fields[0];
        return ids;
    }

    private static AccessibilityNodeProvider emptyProvider() throws Exception {
        Class<?> type = Class.forName(CranposeActivity.class.getName() + "$CranposeAccessibilityProvider");
        Constructor<?> constructor = type.getDeclaredConstructor(View.class);
        constructor.setAccessible(true);
        return (AccessibilityNodeProvider) constructor.newInstance(new View(
                InstrumentationRegistry.getInstrumentation().getTargetContext()));
    }

    /** The controls the provider holds, in tree order. */
    private static List<?> elements(AccessibilityNodeProvider provider) throws Exception {
        Field elements = provider.getClass().getDeclaredField("elements");
        elements.setAccessible(true);
        return (List<?>) elements.get(provider);
    }

    /** The controls a provider holds after taking `records` as its first tree. */
    private static List<?> parse(Record... records) throws Exception {
        AccessibilityNodeProvider provider = emptyProvider();
        assertTrue(update(provider, ids(records), payload(records), new int[0]));
        return elements(provider);
    }

    @Test
    public void readsTextVerbatimAndEmptyFields() throws Exception {
        List<?> elements = parse(
                record(17, "A%09\tB\nC\r\u001f\uD83C\uDF17", "Pause\u001finside", ""),
                record(18, "Empty actions"));
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
    public void aPayloadCutShortKeepsTheRecordsBeforeItAndAsksForEveryRecord() throws Exception {
        byte[] whole = payload(record(20, "Valid"), record(21, "Cut"));
        AccessibilityNodeProvider provider = emptyProvider();
        assertFalse(update(provider, new int[]{20, 21}, Arrays.copyOf(whole, whole.length - 7),
                new int[0]));
        List<?> elements = elements(provider);
        assertEquals(1, elements.size());
        assertEquals(20, field(elements.get(0), "id"));
        assertEquals("Valid", field(elements.get(0), "label"));
        assertEquals(null, provider.createAccessibilityNodeInfo(21));
    }

    @Test
    public void emptyPayloadsHaveNoNodes() throws Exception {
        AccessibilityNodeProvider provider = emptyProvider();
        assertTrue(update(provider, new int[0], null, new int[0]));
        assertTrue(elements(provider).isEmpty());
        assertTrue(update(provider, new int[0], new byte[0], new int[0]));
        assertTrue(elements(provider).isEmpty());
    }

    private static AccessibilityNodeProvider provider(Record... records) throws Exception {
        AccessibilityNodeProvider provider = emptyProvider();
        assertTrue(update(provider, ids(records), payload(records), new int[0]));
        return provider;
    }

    @Test
    public void progressIndicatorsReportTheirRangeWithoutOfferingAdjustment() throws Exception {
        Record loading = record(21, "Loading").set(1, 15).set(20, 40f).set(21, 0f).set(22, 100f);
        AccessibilityNodeInfo node = provider(loading).createAccessibilityNodeInfo(21);
        assertEquals("android.widget.ProgressBar", node.getClassName());
        assertNotNull(node.getRangeInfo());
        assertEquals(40.0f, node.getRangeInfo().getCurrent(), 0.0f);
        assertEquals(100.0f, node.getRangeInfo().getMax(), 0.0f);
        assertFalse(node.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS));
    }

    @Test
    public void disabledControlsRejectActionsButRemainDiscoverable() throws Exception {
        AccessibilityNodeProvider provider = provider(
                record(22, "Disabled", "Remove").set(15, 0));
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
        Record child = record(32, "Title").set(1, 3).set(10, "Café notes").set(26, 31);
        AccessibilityNodeProvider provider = provider(
                record(31, "Notebook"), child, record(33, "Other notes"));
        assertEquals(2, provider.findAccessibilityNodeInfosByText("NOTES", -1).size());
        assertEquals(1, provider.findAccessibilityNodeInfosByText("notes", 31).size());
        assertEquals(1, provider.findAccessibilityNodeInfosByText("CAFÉ", 32).size());
        assertEquals("Notebook", provider.findAccessibilityNodeInfosByText("book", 31)
                .get(0).getContentDescription());
        assertTrue(provider.findAccessibilityNodeInfosByText("notes", 99).isEmpty());
        assertTrue(provider.findAccessibilityNodeInfosByText("absent", -1).isEmpty());
    }

    /**
     * Hands the provider one update as the app writes it: the count of ids
     * and the ids, the count of move numbers and the moves, then the records,
     * in an array longer than the update, as the kept array often is.
     */
    private static boolean update(AccessibilityNodeProvider provider, int[] order, byte[] records,
            int[] moves) throws Exception {
        int recordBytes = records == null ? 0 : records.length;
        ByteBuffer message = ByteBuffer.allocate(8 + 4 * (order.length + moves.length) + recordBytes + 64)
                .order(ByteOrder.LITTLE_ENDIAN);
        message.putInt(order.length);
        for (int id : order) message.putInt(id);
        message.putInt(moves.length);
        for (int number : moves) message.putInt(number);
        if (records != null) message.put(records);
        Method method = provider.getClass().getDeclaredMethod("update", byte[].class, int.class);
        method.setAccessible(true);
        return (Boolean) method.invoke(provider, message.array(), message.position());
    }

    private static Rect bounds(AccessibilityNodeProvider provider, int id) {
        Rect rect = new Rect();
        provider.createAccessibilityNodeInfo(id).getBoundsInParent(rect);
        return rect;
    }

    @Test
    public void updatesKeepUnsentControlsResendChangedOnesAndMoveBounds() throws Exception {
        AccessibilityNodeProvider provider = emptyProvider();
        assertTrue(update(provider, new int[]{41, 42, 43},
                payload(record(41, "One"), record(42, "Two"), record(43, "Three")), new int[0]));

        assertTrue(update(provider, new int[]{43, 41}, payload(record(43, "Renamed")),
                new int[]{41, 5, 6, 7, 8}));

        assertEquals("Renamed", provider.createAccessibilityNodeInfo(43).getContentDescription());
        assertEquals("One", provider.createAccessibilityNodeInfo(41).getContentDescription());
        assertEquals(new Rect(5, 6, 7, 8), bounds(provider, 41));
        assertEquals(new Rect(2, 4, 62, 84), bounds(provider, 43));
        assertEquals(null, provider.createAccessibilityNodeInfo(42));
    }

    @Test
    public void anUpdateNamingAnUnknownControlAsksForEveryRecord() throws Exception {
        AccessibilityNodeProvider provider = provider(record(51, "Kept"));
        assertFalse(update(provider, new int[]{51, 52}, new byte[0], new int[0]));
        assertEquals("Kept", provider.createAccessibilityNodeInfo(51).getContentDescription());
    }

    @Test
    public void aControlJoiningAfterAnotherLeftShowsOnlyItsOwnRecord() throws Exception {
        AccessibilityNodeProvider provider = emptyProvider();
        Record leaving = record(61, "Leaving", "Archive", "Share").set(26, 60);
        assertTrue(update(provider, new int[]{60, 61},
                payload(record(60, "List"), leaving), new int[0]));
        assertTrue(update(provider, new int[]{60, 62},
                payload(record(62, "Second").set(11, "")), new int[0]));
        Record joining = record(63, "Joining").set(11, "").set(12, "").set(36, "");
        assertTrue(update(provider, new int[]{60, 62, 63}, payload(joining), new int[0]));

        AccessibilityNodeInfo node = provider.createAccessibilityNodeInfo(63);
        assertEquals("Joining", node.getContentDescription());
        assertEquals(new Rect(2, 4, 62, 84), bounds(provider, 63));
        for (AccessibilityNodeInfo.AccessibilityAction action : node.getActionList()) {
            assertFalse("no action of the control that left: " + action,
                    "Archive".contentEquals(String.valueOf(action.getLabel()))
                            || "Share".contentEquals(String.valueOf(action.getLabel())));
        }
        assertEquals(null, provider.createAccessibilityNodeInfo(61));
        assertEquals("Second", provider.createAccessibilityNodeInfo(62).getContentDescription());
        assertEquals(0, provider.createAccessibilityNodeInfo(60).getChildCount());
    }

    @Test
    public void aResentControlReadsItsNewTextAndKeepsTheRest() throws Exception {
        AccessibilityNodeProvider provider = provider(record(71, "Price 10", "Buy"));
        assertTrue(update(provider, new int[]{71},
                payload(record(71, "Price 11", "Buy")), new int[0]));
        AccessibilityNodeInfo node = provider.createAccessibilityNodeInfo(71);
        assertEquals("Price 11", node.getContentDescription());
        boolean buy = false;
        for (AccessibilityNodeInfo.AccessibilityAction action : node.getActionList()) {
            buy |= "Buy".contentEquals(String.valueOf(action.getLabel()));
        }
        assertTrue("the custom action survives the resend", buy);
    }
}
