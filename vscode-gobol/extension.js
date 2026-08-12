const vscode = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient');
const path = require('path');
const fs = require('fs');
const os = require('os');

let client = null;

function findGobolLsp() {
    const config = vscode.workspace.getConfiguration('gobol');

    // 1. User-specified path
    const customPath = config.get('lspPath');
    if (customPath && fs.existsSync(customPath)) {
        return { command: customPath, args: [] };
    }

    // 2. Installed in ~/.gobol/bin
    const homeBin = path.join(os.homedir(), '.gobol', 'bin', 'gobol-lsp');
    if (fs.existsSync(homeBin)) {
        return { command: homeBin, args: [] };
    }

    // 3. Project target/release (development)
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        const targetBin = path.join(workspaceFolders[0].uri.fsPath, 'target', 'release', 'gobol-lsp');
        if (fs.existsSync(targetBin)) {
            return { command: targetBin, args: [] };
        }
        // 4. cargo run fallback (slowest, for development)
        if (config.get('cargoRun')) {
            return {
                command: 'cargo',
                args: ['run', '--bin', 'gobol-lsp', '--release'],
                options: { cwd: workspaceFolders[0].uri.fsPath }
            };
        }
    }

    // 5. gobol-lsp in PATH
    return { command: 'gobol-lsp', args: [] };
}

function activate(context) {
    const serverOptions = findGobolLsp();

    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'gobol' }],
        synchronize: {
            configurationSection: 'gobol'
        }
    };

    client = new LanguageClient(
        'gobol-lsp',
        'Gobol Language Server',
        serverOptions,
        clientOptions
    );

    client.start();
}

function deactivate() {
    if (client) {
        return client.stop();
    }
    return undefined;
}

module.exports = { activate, deactivate };
