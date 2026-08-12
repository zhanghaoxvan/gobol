const esbuild = require('esbuild');

esbuild.buildSync({
    entryPoints: ['extension.js'],
    bundle: true,
    outfile: 'out/extension.js',
    platform: 'node',
    external: ['vscode'],
});
