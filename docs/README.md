# REFEREE documentation site

## Cloudflare Workers Builds

Set these values in the Worker's **Settings > Build** configuration:

| Setting | Value |
| --- | --- |
| Root directory | `docs` |
| Build command | `npm run build:cloudflare` |
| Deploy command | `npx opennextjs-cloudflare deploy` |
| Non-production branch deploy command (if enabled) | `npx opennextjs-cloudflare upload` |

The Cloudflare build script runs the Next.js build and then creates the Worker,
assets, and compiled OpenNext configuration in `.open-next`. Running only
`npm run build` produces `.next` and leaves deployment without the compiled
OpenNext configuration.

Keep the `build` script as `next build`: OpenNext invokes it internally.
The dashboard build command must select `build:cloudflare` explicitly.

To verify the deployment build locally without publishing:

```sh
cd docs
npm ci
npm run build:cloudflare
```

For local Workers preview, run `npm run preview`. To build and publish manually,
run `npm run deploy`.

See the [OpenNext setup guide](https://opennext.js.org/cloudflare/get-started)
and [Workers Builds configuration](https://developers.cloudflare.com/workers/ci-cd/builds/configuration/).
