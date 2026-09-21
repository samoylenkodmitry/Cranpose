import argparse
import base64
import json
from pathlib import Path
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


class WebDriver:
    def __init__(self, endpoint, capabilities):
        self.endpoint = endpoint.rstrip('/')
        self.session = ''
        deadline = time.monotonic() + 30
        while True:
            try:
                self.call('GET', '/status')
                break
            except URLError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.1)
        result = self.call('POST', '/session', {'capabilities': {'alwaysMatch': capabilities}})
        self.session = '/session/' + result['sessionId']
        self.capabilities = result['capabilities']

    def call(self, method, path, payload=None):
        data = None if payload is None else json.dumps(payload).encode()
        request = Request(self.endpoint + self.session + path, data=data, method=method,
                          headers={'Content-Type': 'application/json'})
        try:
            with urlopen(request, timeout=90) as response:
                result = json.load(response)
        except HTTPError as error:
            raise RuntimeError(error.read().decode()) from error
        value = result.get('value')
        if isinstance(value, dict) and 'error' in value:
            raise RuntimeError(value)
        return value

    def evaluate(self, expression):
        return self.call('POST', '/execute/sync', {'script': 'return (' + expression + ');', 'args': []})

    def wait(self, expression):
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if self.evaluate(expression):
                return
            time.sleep(0.1)
        raise AssertionError('Timed out: ' + expression)

    def keys(self, text):
        actions = []
        for value in text:
            value = '\ue007' if value == '\n' else value
            actions.extend([{'type': 'keyDown', 'value': value}, {'type': 'keyUp', 'value': value}])
        self.call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': actions}]})

    def click(self, point):
        self.call('POST', '/actions', {'actions': [{
            'type': 'pointer', 'id': 'mouse', 'parameters': {'pointerType': 'mouse'},
            'actions': [
                {'type': 'pointerMove', 'duration': 0, 'origin': 'viewport', **point},
                {'type': 'pointerDown', 'button': 0},
                {'type': 'pointerUp', 'button': 0},
            ],
        }]})

    def navigate(self, url):
        self.call('POST', '/url', {'url': url})

    def close(self):
        self.call('DELETE', '')


def check(driver, report, description, expression):
    result = driver.evaluate(expression)
    if result is not True:
        raise AssertionError(f'{description}: {result!r}')
    report['passed'].append(description)


def record(driver):
    return driver.evaluate("""(() => {
        window.imeField = document.querySelector('input[data-cranpose-node], textarea[data-cranpose-node]');
        window.imeEvents = [];
        for (const type of ['focusin', 'focusout', 'compositionstart', 'compositionupdate', 'compositionend', 'beforeinput', 'input']) {
            document.addEventListener(type, event => window.imeEvents.push({
                type, trusted: event.isTrusted, field: event.target === window.imeField,
                hidden: !!event.target.closest?.('[aria-hidden="true"], [hidden], [inert]'),
                value: event.target.value, inputType: event.inputType, data: event.data,
                start: event.target.selectionStart, end: event.target.selectionEnd,
            }));
        }
        const rect = window.imeField.getBoundingClientRect();
        return {x: Math.round(rect.left + rect.width / 2), y: Math.round(rect.top + rect.height / 2)};
    })()""")


def state_contains(driver, text):
    driver.wait("document.querySelector('[data-cranpose-accessibility]').textContent.includes("
                + json.dumps(text) + ")")


def run(driver, url, report):
    driver.navigate(url + '?tab=textinput')
    driver.wait("!!document.querySelector('input[data-cranpose-node], textarea[data-cranpose-node]')")
    driver.click(record(driver))
    driver.wait('document.activeElement === window.imeField')
    check(driver, report, 'pointer focus reaches the accessible editor',
          "window.imeEvents.some(e => e.type === 'focusin' && e.trusted && e.field)"
          " && window.imeEvents.filter(e => e.type === 'focusin').every(e => !e.hidden)")
    check(driver, report, 'no hidden helper textarea',
          "document.querySelectorAll('textarea:not([data-cranpose-node])').length === 0")
    driver.evaluate('window.imeField.select()')
    driver.keys('A🌍Z')
    state_contains(driver, 'A🌍Z')
    driver.evaluate("window.imeField.setSelectionRange(1, 3, 'backward')")
    driver.wait("window.imeField.selectionDirection === 'backward'")
    driver.keys('日本語')
    state_contains(driver, 'A日本語Z')
    check(driver, report, 'Unicode replacement reaches application state once',
          "window.imeField.value === 'A日本語Z' && window.imeField.selectionStart === 4"
          " && window.imeField.selectionEnd === 4")
    check(driver, report, 'keyboard input is trusted and targets the same editor',
          "window.imeEvents.some(e => e.type === 'input') && window.imeEvents.filter(e => e.type === 'input').every(e => e.trusted && e.field)")
    driver.evaluate('window.imeField.select()')
    driver.keys('first\nsecond')
    state_contains(driver, 'first\nsecond')
    check(driver, report, 'multiline editing preserves the focused control',
          "window.imeField.tagName === 'TEXTAREA' && window.imeField.isConnected"
          " && document.activeElement === window.imeField && window.imeField.value === 'first\\nsecond'")
    driver.keys('\ue004')
    driver.wait("document.activeElement?.getAttribute('aria-label') === 'Empty text field'")
    driver.keys('second field')
    state_contains(driver, 'Field 2 value: "second field"')
    check(driver, report, 'Tab transfers the input session',
          "window.imeField.value === 'first\\nsecond'")
    driver.evaluate("""(() => {
        if (!document.querySelector('[data-cranpose-node][aria-label="Counter App"]')) {
            document.querySelector('[role="combobox"]').click();
        }
    })()""")
    driver.wait("!!document.querySelector('[data-cranpose-node][aria-label=\"Counter App\"]')")
    driver.evaluate("document.querySelector('[data-cranpose-node][aria-label=\"Counter App\"]').click()")
    driver.wait("!!document.querySelector('[data-cranpose-node][aria-label=\"Increment\"]')")
    check(driver, report, 'removing the editor releases focus',
          '!window.imeField.isConnected && document.activeElement !== window.imeField')
    report['text_events'] = driver.evaluate('window.imeEvents')

    driver.navigate(url + '?tab=accessibility_robot')
    driver.wait("!!document.querySelector('[aria-label=\"Passphrase\"]')")
    check(driver, report, 'unfocused password reflects its application value',
          "document.querySelector('[aria-label=\"Passphrase\"]').value === 'robot-secret-value'")
    driver.evaluate("document.querySelector('[aria-label=\"Notes\"]').focus()")
    driver.keys('\ue004')
    driver.wait("document.activeElement?.getAttribute('aria-label') === 'Passphrase'")
    check(driver, report, 'password uses a native protected editor',
          "document.activeElement.type === 'password' && document.activeElement.value === 'robot-secret-value'")
    driver.evaluate('document.activeElement.select()')
    driver.keys('private🌍')
    driver.wait("document.activeElement.value === 'private🌍'")
    driver.evaluate("document.activeElement.setSelectionRange(7, 9, 'backward')")
    time.sleep(0.3)
    check(driver, report, 'password preserves backward Unicode selection',
          "document.activeElement.selectionStart === 7 && document.activeElement.selectionEnd === 9"
          " && document.activeElement.selectionDirection === 'backward'")
    driver.keys('X')
    driver.wait("document.activeElement.value === 'privateX'")
    driver.evaluate("document.querySelector('[aria-label=\"Notes\"]').focus()")
    driver.evaluate("document.querySelector('[aria-label=\"Passphrase\"]').focus()")
    check(driver, report, 'password editing persists across focus changes',
          "document.activeElement.value === 'privateX'")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--endpoint', required=True)
    parser.add_argument('--browser', choices=['firefox', 'safari'], required=True)
    parser.add_argument('--url', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--ios-device')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    caps = {'browserName': args.browser}
    if args.browser == 'firefox':
        caps['moz:firefoxOptions'] = {'prefs': {'webgl.force-enabled': True}}
    if args.ios_device:
        caps.update({'platformName': 'ios', 'safari:deviceUDID': args.ios_device})
    report = {'browser': args.browser, 'status': 'running', 'passed': [],
              'coverage': 'WebDriver keyboard events; no OS IME composition claim'}
    driver = None
    try:
        driver = WebDriver(args.endpoint, caps)
        report['capabilities'] = driver.capabilities
        if not args.ios_device:
            driver.call('POST', '/window/rect', {'width': 1100, 'height': 900})
        run(driver, args.url.rstrip('/'), report)
        report['status'] = 'passed'
    except BaseException as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        if driver:
            try:
                report['final_state'] = driver.evaluate("""({
                    url: location.href, phase: document.body.dataset.phase,
                    active: document.activeElement?.outerHTML,
                    events: window.imeEvents,
                    boot: document.querySelector('#boot-status')?.textContent
                })""")
                screenshot = driver.call('GET', '/screenshot')
                (args.output / 'screen.png').write_bytes(base64.b64decode(screenshot))
            except Exception as error:
                report['artifact_error'] = str(error)
            try:
                driver.close()
            except Exception as error:
                report['cleanup_error'] = str(error)
                report['status'] = 'failed'
        (args.output / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
        print(json.dumps({'browser': report['browser'], 'status': report['status'], 'passed': report['passed']}, indent=2))
    if report['status'] != 'passed':
        raise RuntimeError(report.get('cleanup_error', 'Browser check failed'))


if __name__ == '__main__':
    main()
