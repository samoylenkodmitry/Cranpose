import { dotnet } from './_framework/dotnet.js';

const runtime = await dotnet.withDiagnosticTracing(false).create();
const config = runtime.getConfig();
await runtime.runMain(config.mainAssemblyName, []);
