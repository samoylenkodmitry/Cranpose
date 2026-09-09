package com.compose_rs.demo;

import android.graphics.Rect;

import dev.cranpose.android.CranposeActivity;

import org.junit.Test;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.List;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
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
                "1", label, "", "On", "Toggle", "-1", "1", "1", actions);
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
}
