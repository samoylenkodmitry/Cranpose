import {Buffer} from "node:buffer";
import {writeFileSync} from "node:fs";
import {join} from "node:path";

export async function checkMarkdownImages({send, until, evaluate, report, url, output}) {
    const {identifier} = await send("Page.addScriptToEvaluateOnNewDocument", {
        source: `
    const originalFetch = window.fetch.bind(window);
    window.fetch = async (input, options) => {
      const requested = input instanceof Request ? input.url : String(input);
      const documentUrl = 'https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/_leetcode_source/2023-07-14-leetcode_daily.md';
      const imageUrl = 'https://raw.githubusercontent.com/samoylenkodmitry/s-a--m.github.io/refs/heads/master/markdown-robot.png';
      if (requested === documentUrl) {
        return new Response('![Markdown robot image](/markdown-robot.png)', { status: 200 });
      }
      if (requested === imageUrl) {
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 32;
        const context = canvas.getContext('2d');
        context.fillStyle = '#20dc40';
        context.fillRect(0, 0, 32, 32);
        const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
        return new Response(blob, { status: 200 });
      }
      if (requested.startsWith('https://cranpose-cors-proxy.cranpose.workers.dev/')) {
        return new Response('Upstream not allowed', { status: 403 });
      }
      return originalFetch(input, options);
    };
  `
    });
    const markdownUrl = new URL(url);
    markdownUrl.searchParams.set('tab', 'markdown');
    await send('Page.navigate', {url: markdownUrl.href});
    await until(`!!document.querySelector('[data-cranpose-node][aria-label="Fetch"]')`);
    await evaluate(`document.querySelector('[data-cranpose-node][aria-label="Fetch"]').click()`);
    await until(`[...document.querySelectorAll('[data-cranpose-node]')].some(node =>
    node.textContent === 'Markdown robot image' || node.getAttribute('aria-label') === 'Markdown robot image')`);
    if (output) {
        const screenshot = await send('Page.captureScreenshot');
        writeFileSync(join(output, 'markdown-image.png'), Buffer.from(screenshot.data, 'base64'));
    }
    await send('Page.removeScriptToEvaluateOnNewDocument', {identifier});
    report.push('Markdown loads a CORS-enabled image without the restricted comic proxy');
}
