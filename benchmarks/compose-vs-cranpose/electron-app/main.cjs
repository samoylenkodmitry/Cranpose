const { app, BrowserWindow } = require('electron');

let window;

app.whenReady().then(async () => {
  const page = new URL(app.commandLine.getSwitchValue('page'));
  if (page.protocol !== 'http:' || page.hostname !== '127.0.0.1') {
    throw new Error('The benchmark page must use the local HTTP server');
  }
  window = new BrowserWindow({
    width: 1280,
    height: 820,
    useContentSize: true,
    resizable: false,
    webPreferences: { sandbox: true }
  });
  await window.loadURL(page.href);
});

app.on('window-all-closed', () => app.quit());
