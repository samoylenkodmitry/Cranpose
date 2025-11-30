# Deployment Guide

## GitHub Pages Deployment

The RS-Compose web demo can be automatically deployed to GitHub Pages when you create a new release tag.

### Setup Instructions

#### 1. Enable GitHub Pages

1. Go to your repository on GitHub
2. Click **Settings** → **Pages**
3. Under "Source", select **GitHub Actions**
4. Save the settings

#### 2. Create a Release

The deployment automatically triggers when you push a version tag:

```bash
# Create and push a tag
git tag v0.1.0
git push origin v0.1.0
```

Or create a release through GitHub UI:
1. Go to **Releases** → **Create a new release**
2. Choose a tag (e.g., `v0.1.0`) or create a new one
3. Fill in release notes
4. Click **Publish release**

#### 3. Monitor Deployment

1. Go to **Actions** tab in your repository
2. You'll see the "Deploy to GitHub Pages" workflow running
3. Once complete, your demo will be live at:
   ```
   https://<username>.github.io/<repository-name>/
   ```

For this repository:
```
https://samoylenkodmitry.github.io/rs-compose/
```

### Manual Deployment

You can also trigger deployment manually without creating a tag:

1. Go to **Actions** tab
2. Select "Deploy to GitHub Pages" workflow
3. Click **Run workflow**
4. Select the branch and click **Run workflow**

### How It Works

The workflow (`.github/workflows/deploy-pages.yml`) performs the following steps:

1. **Checkout** - Clones the repository
2. **Install Rust** - Sets up Rust toolchain with wasm32-unknown-unknown target
3. **Cache** - Caches Rust dependencies for faster builds
4. **Install wasm-pack** - Installs the WASM build tool
5. **Build** - Compiles the desktop-demo to WebAssembly
6. **Deploy** - Uploads and publishes to GitHub Pages

### Build Time

- First build: ~5-10 minutes (no cache)
- Subsequent builds: ~2-3 minutes (with cache)

### Troubleshooting

**Pages not showing up?**
- Check that GitHub Pages source is set to "GitHub Actions" in Settings
- Verify the workflow completed successfully in the Actions tab
- It may take a few minutes for changes to propagate

**Build failing?**
- Check the Actions tab for detailed error logs
- Ensure all dependencies are correctly specified
- Verify wasm-pack compatibility with your Rust version

**404 errors for assets?**
- Ensure `.nojekyll` file is created (workflow handles this)
- Check that pkg/ directory is properly copied to _site/

### Local Testing

Before deploying, test the build locally:

```bash
# Build WASM
wasm-pack build --target web --out-dir apps/desktop-demo/pkg apps/desktop-demo

# Serve locally (requires Python 3 or another HTTP server)
cd apps/desktop-demo
python3 -m http.server 8080

# Or with Node.js
npx serve .
```

Then open http://localhost:8080 in your browser.

### Custom Domain (Optional)

To use a custom domain:

1. Add a `CNAME` file to the deployment in the workflow:
   ```yaml
   - name: Create deployment directory
     run: |
       mkdir -p _site
       cp -r apps/desktop-demo/pkg _site/
       cp apps/desktop-demo/index.html _site/
       echo "your-domain.com" > _site/CNAME
       touch _site/.nojekyll
   ```

2. Configure DNS settings with your domain provider
3. Update GitHub Pages settings with your custom domain

### Security

The workflow uses minimal permissions:
- `contents: read` - Read repository contents
- `pages: write` - Write to GitHub Pages
- `id-token: write` - Required for Pages deployment

No secrets are needed for basic deployment.
