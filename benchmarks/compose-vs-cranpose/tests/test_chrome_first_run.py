"""Chrome on a phone against a stand-in `adb`: `reset_chrome` passes the
screens of Chrome's first run that newer Chrome shows (the choice of a
search engine, then the question about notifications), and a page's
surface is found under the name newer Android gives it."""

import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))

import measure  # noqa: E402

# Serves the screens in $SCREENS one after another, each tap moving to the
# next, and the layers in $LAYERS one listing a call; records every call.
FAKE_ADB = """#!/bin/sh
echo "$*" >> "$ADB_CALLS"
step=$(cat "$ADB_CALLS.step" 2>/dev/null || echo 0)
case "$*" in
  *" pm clear "*) echo Success ;;
  *" input tap "*) echo $((step + 1)) > "$ADB_CALLS.step" ;;
  *" cat /sdcard/window.xml"*) cat "$SCREENS/$step.xml" ;;
  *" dumpsys SurfaceFlinger --list"*)
    listing=$(cat "$ADB_CALLS.listing" 2>/dev/null || echo 0)
    echo $((listing + 1)) > "$ADB_CALLS.listing"
    cat "$LAYERS/$listing.txt" 2>/dev/null || cat "$LAYERS/last.txt" ;;
esac
"""


def view(resource_id, bounds, text='', enabled='true', cls='android.widget.TextView'):
    return (f'<node index="0" text="{text}" resource-id="{resource_id}" class="{cls}" '
            f'package="com.android.chrome" clickable="true" enabled="{enabled}" bounds="{bounds}" />')


def engine(text, bounds):
    return (f'<node index="0" text="{text}" resource-id="" class="android.widget.LinearLayout" '
            f'package="com.android.chrome" clickable="true" enabled="true" bounds="{bounds}" />')


def screen(*views):
    return '<?xml version="1.0" ?><hierarchy rotation="0">' + ''.join(views) + '</hierarchy>'


CHOOSER_TITLE = view('com.android.chrome:id/chooser_title', '[720,180][2074,260]', 'Choose your search engine')
ENGINE_CHOICE = view('com.android.chrome:id/default_search_engine_dialog_options', '[720,299][2074,455]')
FIRST_ENGINE = engine('Google', '[720,299][2074,377]')
SECOND_ENGINE = engine('Other', '[720,377][2074,455]')
# The screens of Chrome's first run on a Galaxy Tab S7+, with the bounds its dump gives.
SCREENS = [
    screen(CHOOSER_TITLE, ENGINE_CHOICE, FIRST_ENGINE, SECOND_ENGINE,
           view('com.android.chrome:id/button_primary', '[1859,1508][2064,1586]', 'Set as default', 'false')),
    screen(CHOOSER_TITLE, ENGINE_CHOICE, FIRST_ENGINE, SECOND_ENGINE,
           view('com.android.chrome:id/button_primary', '[1859,1508][2064,1586]', 'Set as default')),
    screen(view('com.android.chrome:id/notification_permission_rationale_title', '[1036,926][1616,982]'),
           view('com.android.chrome:id/negative_button', '[1443,1088][1586,1166]', 'No thanks'),
           view('com.android.chrome:id/positive_button', '[1599,1088][1764,1166]', 'Continue')),
    screen(view('com.android.chrome:id/url_bar', '[100,80][2700,160]')),
]


class ChromeFirstRunTest(unittest.TestCase):
    def run_with(self, screens, layers, act):
        with tempfile.TemporaryDirectory() as folder:
            tools, calls = Path(folder), Path(folder) / 'calls'
            (tools / 'screens').mkdir()
            for index, content in enumerate(screens):
                (tools / 'screens' / f'{index}.xml').write_text(content)
            (tools / 'layers').mkdir()
            for name, content in layers.items():
                (tools / 'layers' / f'{name}.txt').write_text(content)
            adb = tools / 'adb'
            adb.write_text(FAKE_ADB)
            adb.chmod(adb.stat().st_mode | stat.S_IEXEC)
            environment = {'PATH': f'{tools}{os.pathsep}{os.environ["PATH"]}', 'ADB_CALLS': str(calls),
                           'SCREENS': str(tools / 'screens'), 'LAYERS': str(tools / 'layers')}
            with patch.dict(os.environ, environment), patch.object(measure.time, 'sleep'):
                result = act(measure.Device('SERIAL'))
            return result, calls.read_text().splitlines()

    def test_the_engine_choice_and_the_notification_question_are_passed(self):
        _, lines = self.run_with(SCREENS, {}, measure.reset_chrome)
        taps = [line.split('input tap ')[1] for line in lines if 'input tap ' in line]
        # The first engine and "Set as default" together, then "No thanks".
        self.assertEqual(taps, ['1397 338', '1961 1547', '1514 1127'], lines)
        self.assertEqual(lines[-1], '-s SERIAL shell am force-stop com.android.chrome', lines)

    def test_a_page_surface_named_by_its_layer_id_is_found_once_it_is_there(self):
        layers = {
            '0': 'RequestedLayerState{com.android.chrome/com.google.android.apps.chrome.Main#62777 parentId=1}\n',
            'last': ('RequestedLayerState{com.android.chrome/com.google.android.apps.chrome.Main#62777 parentId=1}\n'
                     'RequestedLayerState{com.android.chrome/ChromeChildSurface#62791 parentId=62789}\n'),
        }
        layer, _ = self.run_with([], layers, measure.PageTarget(None, 'cranpose', 8).layer)
        self.assertEqual(layer, 'com.android.chrome/ChromeChildSurface#62791')

    def test_the_older_surface_name_is_found_as_before(self):
        layers = {'last': 'com.android.chrome/ChromeChildSurface#0\n'}
        layer, _ = self.run_with([], layers, measure.PageTarget(None, 'cranpose', 8).layer)
        self.assertEqual(layer, 'com.android.chrome/ChromeChildSurface#0')


if __name__ == '__main__':
    unittest.main()
