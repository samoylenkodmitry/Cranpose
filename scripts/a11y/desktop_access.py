import json
from pathlib import Path
import subprocess
import sys
import time


class NativeAdapter:
    def snapshot(self):
        return [{key: value for key, value in node.items() if key != 'native'} for node in self.nodes()]


class MacAdapter(NativeAdapter):
    def __init__(self, pid, timeout=1.0):
        import ApplicationServices as ax
        self.ax = ax
        if not ax.AXIsProcessTrusted():
            raise RuntimeError('The macOS robot process needs Accessibility permission')
        error = ax.AXUIElementSetMessagingTimeout(ax.AXUIElementCreateSystemWide(), timeout)
        if error:
            raise RuntimeError(f'Could not set the macOS accessibility timeout: {error}')
        self.root = ax.AXUIElementCreateApplication(pid)

    def attribute(self, node, name):
        error, value = self.ax.AXUIElementCopyAttributeValue(node, name, None)
        return value if error == 0 else None

    def nodes(self):
        pending = [self.root]
        result = []
        seen = set()
        while pending:
            native = pending.pop()
            key = hash(native)
            if key in seen:
                continue
            seen.add(key)
            pending.extend(reversed(self.attribute(native, 'AXChildren') or []))
            role = self.attribute(native, 'AXRole')
            value = self.attribute(native, 'AXValue')
            name = self.attribute(native, 'AXTitle') or self.attribute(native, 'AXDescription') or ''
            if role == 'AXStaticText' and not name:
                name = value or ''
            result.append({'native': native, 'name': str(name),
                           'role': {'AXProgressIndicator': 'progress', 'AXSlider': 'slider'}.get(role, role),
                           'value': value if isinstance(value, (str, int, float)) else None,
                           'enabled': self.attribute(native, 'AXEnabled') is not False})
        return result

    def activate(self, node):
        error, actions = self.ax.AXUIElementCopyActionNames(node['native'], None)
        if error or 'AXPress' not in (actions or []):
            return False
        return self.ax.AXUIElementPerformAction(node['native'], 'AXPress') == 0

    def set_value(self, node, value):
        return self.ax.AXUIElementSetAttributeValue(node['native'], 'AXValue', value) == 0


class LinuxAdapter(NativeAdapter):
    def __init__(self, pid):
        import gi
        gi.require_version('Atspi', '2.0')
        from gi.repository import Atspi, GLib
        self.api = Atspi
        self.error_type = GLib.Error
        self.pid = pid
        Atspi.set_timeout(1000, 1000)
        subprocess.run(['busctl', '--user', 'set-property', 'org.a11y.Bus', '/org/a11y/bus',
                        'org.a11y.Status', 'IsEnabled', 'b', 'true'], check=True, timeout=10)

    def nodes(self):
        for attempt in range(3):
            try:
                return self._nodes()
            except self.error_type as error:
                removed = error.message.startswith('Unknown object ') or (
                    error.domain == 'atspi_error' and error.code == 1
                    and error.message.startswith('/org/a11y/atspi/accessible/'))
                if attempt == 2 or not removed:
                    raise
                time.sleep(0.02)

    def _nodes(self):
        desktop = self.api.get_desktop(0)
        pending = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
        pending = [node for node in pending if node.get_process_id() == self.pid]
        result = []
        while pending:
            native = pending.pop()
            pending.extend(native.get_child_at_index(i) for i in reversed(range(native.get_child_count())))
            interfaces = native.get_interfaces()
            value = native.get_value_iface().get_current_value() if 'Value' in interfaces else None
            if 'Text' in interfaces:
                value = self.api.Text.get_text(native, 0, -1)
            role = native.get_role_name()
            result.append({'native': native, 'name': native.get_name(),
                           'description': native.get_description(),
                           'role': {'progress bar': 'progress', 'slider': 'slider'}.get(role, role),
                           'value': value,
                           'enabled': native.get_state_set().contains(self.api.StateType.ENABLED)})
        return result

    def activate(self, node):
        native = node['native']
        if 'Action' not in native.get_interfaces():
            return False
        action = native.get_action_iface()
        for index in range(action.get_n_actions()):
            if action.get_action_name(index) in ('click', 'press', 'activate'):
                return action.do_action(index)
        return False

    def set_value(self, node, value):
        native = node['native']
        if isinstance(value, str):
            return native.get_editable_text_iface().set_text_contents(value)
        return native.get_value_iface().set_current_value(value)


class WindowsAdapter(NativeAdapter):
    def __init__(self, pid):
        self.pid = pid

    def command(self, operation, name='', value=''):
        script = Path(__file__).with_name('windows_access.ps1')
        output = subprocess.check_output(
            ['powershell.exe', '-NoProfile', '-File', str(script), '-AppProcessId', str(self.pid),
             '-Operation', operation, '-Name', name, '-Value', str(value)], text=True, timeout=20)
        return json.loads(output)

    def nodes(self):
        return self.command('tree')

    def activate(self, node):
        return self.command('activate', node['name'])

    def set_value(self, node, value):
        return self.command('value', node['name'], value)


def adapter_for_platform(pid):
    adapters = {'darwin': MacAdapter, 'linux': LinuxAdapter, 'win32': WindowsAdapter}
    return adapters[sys.platform](pid)
