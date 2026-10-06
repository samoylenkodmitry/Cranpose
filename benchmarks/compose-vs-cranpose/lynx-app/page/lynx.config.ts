import { pluginReactLynx } from '@lynx-js/react-rsbuild-plugin';
import { defineConfig } from '@lynx-js/rspeedy';
import { pluginTypeCheck } from '@rsbuild/plugin-type-check';

// One entry, `src/index.tsx`, bundled as `dist/main.lynx.bundle` for the
// Android app's assets.
export default defineConfig({
  plugins: [pluginReactLynx(), pluginTypeCheck()],
});
