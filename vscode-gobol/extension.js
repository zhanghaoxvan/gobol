const vscode = require('vscode');
const { LanguageClient, TransportKind, RevealOutputChannelOn } = require('vscode-languageclient/node');
const path = require('path');
const fs = require('fs');
const os = require('os');
const { spawn } = require('child_process');

let client = null;
let statusBarItem = null;
let lspCfgCache = null;
let startingPromise = null;
let projectInfo = null;
let projectWatcher = null;

// ============ Cross-file auto-highlight ============
// Tracks which editors' decorations are active. The LSP's custom
// `gobol/highlights` request returns highlight ranges for the identifier under
// the cursor across *all* open documents; on cursor move / content change we
// re-query and re-decorate every opened gobol editor.
let highlightDecoration = null;        // decoration style object
let highlightEditorMap = new Map();    // editor -> its decoration (for cleanup)
let hlRequestSeq = 0;                  // monotonic id to drop stale highlight responses
let hlDebounceTimer = null;            // debounce timer for refresh

function setStatus(status, tooltip) {
    if (!statusBarItem) return;
    switch (status) {
        case 'starting':
            statusBarItem.text = '$(sync~spin) Gobol LSP starting…';
            statusBarItem.color = new vscode.ThemeColor('statusBarItem.warningForeground');
            break;
        case 'running':
            statusBarItem.text = '$(check) Gobol LSP';
            statusBarItem.color = new vscode.ThemeColor('statusBarItem.prominentForeground');
            break;
        case 'error':
            statusBarItem.text = '$(error) Gobol LSP stopped';
            statusBarItem.color = new vscode.ThemeColor('statusBarItem.errorForeground');
            break;
        case 'stopped':
        default:
            statusBarItem.text = '$(server) Gobol LSP';
            statusBarItem.color = undefined;
            break;
    }
    statusBarItem.tooltip = tooltip || '';
    statusBarItem.show();
}

// ============ Cross-file auto-highlight: implementation ============

function ensureHighlightDecoration() {
    if (!highlightDecoration) {
        highlightDecoration = vscode.window.createTextEditorDecorationType({
            backgroundColor: new vscode.ThemeColor('editor.wordHighlightBackground'),
            border: new vscode.ThemeColor('editor.wordHighlightStrongBorder'),
        });
    }
    return highlightDecoration;
}

// Remove all cross-file highlight decorations from every editor we applied.
// Editors may have been closed since (map entries can out-live the editor), so
// skip any that are no longer open and guard against setDecorations errors.
function clearAllHighlights() {
    for (const editor of highlightEditorMap.keys()) {
        if (!editor.document || editor.document.isClosed) continue;
        try {
            editor.setDecorations(highlightDecoration, []);
        } catch (_) {
            // editor already disposed / invalid; drop it silently
        }
    }
    highlightEditorMap.clear();
}

// Apply cross-file highlight ranges to the given editor.
function applyHighlights(editor, ranges) {
    ensureHighlightDecoration();
    editor.setDecorations(highlightDecoration, ranges);
    highlightEditorMap.set(editor, ranges);
}

// Debounced, sequenced refresh of the cross-file highlights. The public entry
// point called by VS Code events; coalesces rapid cursor/content changes into
// a single request and drops responses that are no longer the latest.
function refreshCrossFileHighlights() {
    clearTimeout(hlDebounceTimer);
    hlDebounceTimer = setTimeout(() => {
        performHighlightRefresh();
    }, 120);
}

async function performHighlightRefresh() {
    const seq = ++hlRequestSeq;
    const active = vscode.window.activeTextEditor;
    if (!client || !client.isRunning() || !active || active.document.languageId !== 'gobol') {
        clearAllHighlights();
        return;
    }
    const params = {
        textDocument: { uri: active.document.uri.toString() },
        position: active.selection.active,
    };
    // Always clear first so stale highlights never linger if the request
    // fails or travels to a client that no longer has the document open.
    clearAllHighlights();
    try {
        // Filter out per-file entries that are empty to avoid setDecorations
        // no-ops, and only touch editors that are still open.
        const perFile = await client.sendRequest('gobol/highlights', params);
        if (seq !== hlRequestSeq) return; // a newer request has superseded us
        for (const item of perFile || []) {
            if (!item.highlights || item.highlights.length === 0) continue;
            const docUri = vscode.Uri.parse(item.uri);
            for (const ed of vscode.window.visibleTextEditors) {
                if (ed.document.languageId === 'gobol' && ed.document.uri.toString() === docUri.toString()) {
                    const ranges = item.highlights.map(h =>
                        new vscode.Range(
                            h.range.start.line, h.range.start.character,
                            h.range.end.line, h.range.end.character
                        )
                    );
                    applyHighlights(ed, ranges);
                }
            }
        }
    } catch (_) {
        // Server may have restarted or the request may legitimately fail;
        // highlights were already cleared so the state stays consistent.
    }
}

// ============ grape.toml detection and parsing ============

// Minimal TOML parser tailored to the grape.toml schema.
// Supports: tables [section], key = value, strings, arrays, inline tables,
// booleans, and numbers.
function parseToml(content) {
    const root = {};
    let current = root;
    for (const rawLine of content.split(/\r?\n/)) {
        let line = rawLine.trim();
        // strip inline comments (naive; fine for grape.toml)
        const hash = findCommentStart(line);
        if (hash >= 0) line = line.slice(0, hash).trim();
        if (line === '') continue;

        // table header [section] or [a.b]
        const header = line.match(/^\[(.+)\]$/);
        if (header) {
            const parts = header[1].split('.').map(s => unquote(s.trim()));
            let node = root;
            for (const part of parts) {
                node[part] = node[part] || {};
                node = node[part];
            }
            current = node;
            continue;
        }

        // key = value
        const eq = line.indexOf('=');
        if (eq < 0) continue;
        const key = unquote(line.slice(0, eq).trim());
        current[key] = parseTomlValue(line.slice(eq + 1).trim());
    }
    return root;
}

// Find the start of an inline comment (#) that is not inside a string.
function findCommentStart(line) {
    let inStr = false, quote = '';
    for (let i = 0; i < line.length; i++) {
        const c = line[i];
        if (inStr) {
            if (c === '\\') { i++; continue; }
            if (c === quote) inStr = false;
        } else {
            if (c === '"' || c === "'") { inStr = true; quote = c; }
            else if (c === '#') return i;
        }
    }
    return -1;
}

function unquote(s) {
    if ((s[0] === '"' && s[s.length - 1] === '"') ||
        (s[0] === "'" && s[s.length - 1] === "'")) {
        return s.slice(1, -1);
    }
    return s;
}

function parseBasicString(s) {
    let out = '';
    for (let i = 1; i < s.length; i++) {
        const c = s[i];
        if (c === '\\' && i + 1 < s.length) {
            const n = s[++i];
            out += ({ n: '\n', t: '\t', r: '\r', '"': '"', '\\': '\\' })[n] || n;
        } else if (c === '"') break;
        else out += c;
    }
    return out;
}

function parseTomlArray(s) {
    const arr = [];
    let inner = s.slice(1).trim();
    while (inner[0] !== ']') {
        const parsed = parseTomlValueWithRest(inner);
        arr.push(parsed.value);
        inner = parsed.rest.trim();
        if (inner[0] === ',') inner = inner.slice(1).trim();
    }
    return arr;
}

function parseTomlInlineTable(s) {
    const tbl = {};
    let inner = s.slice(1).trim();
    while (inner[0] !== '}') {
        const eq = inner.indexOf('=');
        const key = unquote(inner.slice(0, eq).trim());
        const parsed = parseTomlValueWithRest(inner.slice(eq + 1).trim());
        tbl[key] = parsed.value;
        inner = parsed.rest.trim();
        if (inner[0] === ',') inner = inner.slice(1).trim();
    }
    return tbl;
}

// Parse a TOML value and return { value, rest } where rest is the unconsumed
// portion of the input string (used for arrays and inline tables).
function parseTomlValueWithRest(s) {
    if (s[0] === '"') {
        let i = 1;
        while (i < s.length) {
            if (s[i] === '\\') { i += 2; continue; }
            if (s[i] === '"') break;
            i++;
        }
        return { value: parseBasicString(s), rest: s.slice(i + 1) };
    }
    if (s[0] === "'") {
        const end = s.indexOf("'", 1);
        return { value: s.slice(1, end), rest: s.slice(end + 1) };
    }
    if (s[0] === '[') {
        // find matching closing bracket
        let depth = 0;
        for (let i = 0; i < s.length; i++) {
            if (s[i] === '[') depth++;
            else if (s[i] === ']') { depth--; if (depth === 0) return { value: parseTomlArray(s.slice(0, i + 1)), rest: s.slice(i + 1) }; }
        }
    }
    if (s[0] === '{') {
        let depth = 0;
        for (let i = 0; i < s.length; i++) {
            if (s[i] === '{') depth++;
            else if (s[i] === '}') { depth--; if (depth === 0) return { value: parseTomlInlineTable(s.slice(0, i + 1)), rest: s.slice(i + 1) }; }
        }
    }
    // scalar: read until comma or closing bracket
    let end = s.length;
    for (let i = 0; i < s.length; i++) {
        if (s[i] === ',' || s[i] === ']' || s[i] === '}') { end = i; break; }
    }
    const raw = s.slice(0, end).trim();
    let value = raw;
    if (raw === 'true') value = true;
    else if (raw === 'false') value = false;
    else { const n = Number(raw); if (!isNaN(n)) value = n; }
    return { value, rest: s.slice(end) };
}

// Override parseTomlValue to use the rest-aware version for top-level values.
function parseTomlValue(s) {
    return parseTomlValueWithRest(s).value;
}

// Search for grape.toml in the workspace root (and first workspace folder).
function findGrapeToml() {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) return null;
    const root = folders[0].uri.fsPath;
    const candidate = path.join(root, 'grape.toml');
    return fs.existsSync(candidate) ? candidate : null;
}

// Load and normalize a grape.toml file into a project info object.
function loadProject(tomlPath) {
    try {
        const content = fs.readFileSync(tomlPath, 'utf8');
        const data = parseToml(content);
        const proj = data.project || {};
        const deps = data.dependencies || {};
        const dependencies = {};
        for (const [name, spec] of Object.entries(deps)) {
            if (typeof spec === 'object') {
                dependencies[name] = {
                    repo: spec.repo || '',
                    tag: spec.tag || '',
                    optional: spec.optional || false,
                };
            }
        }
        return {
            root: path.dirname(tomlPath),
            tomlPath,
            name: proj.name || '(unnamed)',
            version: proj.version || '0.0.0',
            entry: proj.entry || 'main.gbl',
            authors: proj.authors || [],
            description: proj.description || null,
            license: proj.license || null,
            dependencies,
        };
    } catch (err) {
        vscode.window.showWarningMessage(`Gobol: failed to parse grape.toml: ${err.message}`);
        return null;
    }
}

// Refresh project info from grape.toml and update the status bar / context.
function refreshProject(context) {
    // dispose previous watcher
    if (projectWatcher) { projectWatcher.dispose(); projectWatcher = null; }

    const tomlPath = findGrapeToml();
    if (!tomlPath) {
        projectInfo = null;
        vscode.commands.executeCommand('setContext', 'gobol:hasProject', false);
        return null;
    }

    projectInfo = loadProject(tomlPath);
    vscode.commands.executeCommand('setContext', 'gobol:hasProject', !!projectInfo);

    if (projectInfo) {
        // Watch grape.toml for changes so project info stays in sync.
        const pattern = new vscode.RelativePattern(
            vscode.workspace.workspaceFolders[0],
            'grape.toml'
        );
        projectWatcher = vscode.workspace.createFileSystemWatcher(pattern);
        projectWatcher.onDidChange(() => { projectInfo = loadProject(tomlPath); });
        projectWatcher.onDidCreate(() => { projectInfo = loadProject(tomlPath); });
        projectWatcher.onDidDelete(() => {
            projectInfo = null;
            vscode.commands.executeCommand('setContext', 'gobol:hasProject', false);
        });
        if (context) context.subscriptions.push(projectWatcher);
    }
    return projectInfo;
}

function showProject() {
    if (!projectInfo) {
        vscode.window.showInformationMessage('Gobol: No grape.toml found in the current workspace.');
        return;
    }
    const depCount = Object.keys(projectInfo.dependencies).length;
    const lines = [
        `Project:  ${projectInfo.name} v${projectInfo.version}`,
        `Entry:    ${projectInfo.entry}`,
        `Root:     ${projectInfo.root}`,
        `License:  ${projectInfo.license || '(none)'}`,
        `Deps:     ${depCount}`,
    ];
    for (const [name, spec] of Object.entries(projectInfo.dependencies)) {
        lines.push(`  ${name} @ ${spec.tag} (${spec.repo})`);
    }
    vscode.window.showInformationMessage(lines.join('\n'), { modal: false });
}

function findGobolLsp() {
    const config = vscode.workspace.getConfiguration('gobol');

    // 1. User-specified path
    const customPath = config.get('lspPath');
    if (customPath && typeof customPath === 'string' && customPath.length > 0 && fs.existsSync(customPath)) {
        return { command: customPath, args: [], method: 'config', path: customPath };
    }

    // 2. Installed in ~/.gobol/bin
    const homeBin = path.join(os.homedir(), '.gobol', 'bin', 'gobol-lsp');
    if (fs.existsSync(homeBin)) {
        return { command: homeBin, args: [], method: 'user-home', path: homeBin };
    }

    // 3. Project target/debug (development — faster rebuild)
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        const root = workspaceFolders[0].uri.fsPath;
        const debugBin = path.join(root, 'target', 'debug', 'gobol-lsp');
        if (fs.existsSync(debugBin)) {
            return { command: debugBin, args: [], method: 'target-debug', path: debugBin };
        }

        const releaseBin = path.join(root, 'target', 'release', 'gobol-lsp');
        if (fs.existsSync(releaseBin)) {
            return { command: releaseBin, args: [], method: 'target-release', path: releaseBin };
        }

        // cargo run fallback (slowest, for development)
        if (config.get('cargoRun')) {
            return {
                command: 'cargo',
                args: ['run', '--bin', 'gobol-lsp', '--release'],
                options: { cwd: root },
                method: 'cargo-run',
                path: `cargo run (cwd=${root})`,
            };
        }
    }

    // 5. gobol-lsp in PATH
    return { command: 'gobol-lsp', args: [], method: 'PATH', path: 'gobol-lsp (from PATH)' };
}

function buildClientOptions() {
    const traceLevel = vscode.workspace.getConfiguration('gobol.trace').get('server', 'off');
    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'gobol' }],
        synchronize: {
            configurationSection: 'gobol'
        },
        revealOutputChannelOn: RevealOutputChannelOn.Error,
        outputChannelName: 'Gobol Language Server',
        traceOutputChannel: traceLevel !== 'off'
            ? vscode.window.createOutputChannel('Gobol LSP Trace')
            : undefined,
        middleware: {
            handleDiagnostics: (uri, diagnostics, next) => {
                next(uri, diagnostics);
                if (diagnostics.length > 0) {
                    setStatus('running', `${diagnostics.length} issue(s) in ${uri.fsPath.split('/').pop()}`);
                }
            },
        },
    };
    return clientOptions;
}

async function startClient() {
    if (startingPromise) return startingPromise;
    startingPromise = (async () => {
        setStatus('starting');
        lspCfgCache = findGobolLsp();

        const serverOptions = () => {
            return new Promise((resolve, reject) => {
                try {
                    const child = spawn(
                        lspCfgCache.command,
                        lspCfgCache.args,
                        lspCfgCache.options ?? { cwd: process.cwd() }
                    );

                    child.on('error', (err) => {
                        vscode.window.showErrorMessage(
                            `Gobol LSP failed to start (${lspCfgCache.method}): ${err.message}`
                        );
                        setStatus('error', `Failed to start: ${err.message}`);
                        reject(err);
                    });

                    child.on('exit', (code, signal) => {
                        if (code !== null && code !== 0) {
                            vscode.window.showWarningMessage(
                                `Gobol LSP exited with code ${code} (signal: ${signal ?? 'none'}). Path: ${lspCfgCache.path}`
                            );
                            setStatus('error', `Exited (code ${code})`);
                        } else if (signal) {
                            setStatus('stopped', `Stopped (signal ${signal})`);
                        } else {
                            setStatus('stopped');
                        }
                    });

                    resolve(child);
                } catch (err) {
                    setStatus('error', err?.message ?? 'unknown spawn error');
                    reject(err);
                }
            });
        };

        client = new LanguageClient(
            'gobol-lsp',
            'Gobol Language Server',
            serverOptions,
            buildClientOptions()
        );

        try {
            await client.start();
            client.onReady().then(() => {
                setStatus('running', `Running via ${lspCfgCache.method} — ${lspCfgCache.path}`);
            }).catch((e) => {
                setStatus('error', `onReady failed: ${e?.message ?? e}`);
            });
            return client;
        } catch (err) {
            setStatus('error', err?.message ?? 'unknown error during start');
            client = null;
            throw err;
        } finally {
            startingPromise = null;
        }
    })();
    return startingPromise;
}

async function stopClient() {
    if (client) {
        try {
            const old = client;
            client = null;
            await old.stop();
        } catch (_) {
            // ignore stop errors
        }
    }
    setStatus('stopped');
}

async function restartLsp() {
    await stopClient();
    try {
        await startClient();
        vscode.window.showInformationMessage(`Gobol LSP restarted (${lspCfgCache.method}).`);
    } catch (err) {
        vscode.window.showErrorMessage(`Restart failed: ${err?.message ?? err}`);
    }
}

function showStatus() {
    if (!lspCfgCache) {
        vscode.window.showInformationMessage('Gobol LSP has not been resolved yet — open a .gbl file first.');
        return;
    }
    const state = client ? (client.isRunning() ? 'running' : 'starting/stopping') : 'stopped';
    vscode.window.showInformationMessage(
        `Gobol LSP: ${state}\nMethod: ${lspCfgCache.method}\nPath: ${lspCfgCache.path}`,
        { modal: false }
    );
}

function activate(context) {
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 10);
    statusBarItem.command = 'gobol.showStatus';
    setStatus('stopped');

    // Detect grape.toml in the workspace root. This also sets the
    // `gobol:hasProject` context key and registers a file-system watcher so
    // the project info stays in sync when grape.toml is edited.
    const info = refreshProject(context);
    if (info) {
        vscode.window.showInformationMessage(
            `Gobol: Detected project ${info.name} v${info.version} (entry: ${info.entry})`
        );
    }

    context.subscriptions.push(
        statusBarItem,
        vscode.commands.registerCommand('gobol.restartLsp', restartLsp),
        vscode.commands.registerCommand('gobol.showStatus', showStatus),
        vscode.commands.registerCommand('gobol.showProject', showProject),
        // Cross-file auto-highlight: re-query whenever the cursor moves within
        // a gobol file, the active gobol editor changes, a gobol document is
        // edited/saved, or the set of visible gobol editors changes.
        vscode.window.onDidChangeTextEditorSelection((e) => {
            if (e.textEditor.document.languageId === 'gobol') {
                refreshCrossFileHighlights();
            }
        }),
        vscode.window.onDidChangeActiveTextEditor((ed) => {
            if (ed && ed.document.languageId === 'gobol') {
                refreshCrossFileHighlights();
            } else {
                clearAllHighlights();
            }
        }),
        vscode.workspace.onDidChangeTextDocument((e) => {
            if (e.document.languageId === 'gobol') {
                refreshCrossFileHighlights();
            }
        }),
        vscode.window.onDidChangeVisibleTextEditors(() => {
            if (vscode.window.activeTextEditor &&
                vscode.window.activeTextEditor.document.languageId === 'gobol') {
                refreshCrossFileHighlights();
            }
        }),
        { dispose: () => { clearTimeout(hlDebounceTimer); clearAllHighlights(); } },
    );

    // Kick off client when a Gobol file is visible, or immediately if one
    // exists, or when a grape.toml project is detected (the LSP can provide
    // diagnostics for the entry file even before it is opened).
    const kick = () => {
        if (client || startingPromise) return;
        const editor = vscode.window.activeTextEditor;
        if (editor && editor.document.languageId === 'gobol') {
            startClient().catch(() => { /* error already surfaced */ });
            return;
        }
        // Auto-start when a grape.toml project exists in the workspace.
        if (projectInfo) {
            startClient().catch(() => { /* error already surfaced */ });
        }
    };
    context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(kick),
        vscode.workspace.onDidOpenTextDocument((doc) => {
            if (doc.languageId === 'gobol' && !client && !startingPromise) {
                startClient().catch(() => { /* error already surfaced */ });
            }
        }),
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('gobol.lspPath') || e.affectsConfiguration('gobol.cargoRun')) {
                vscode.window.showInformationMessage('Gobol LSP configuration changed. Run "Gobol: Restart Language Server" to apply.');
            }
        }),
        // Re-detect grape.toml when workspace folders change (e.g. multi-root).
        vscode.workspace.onDidChangeWorkspaceFolders(() => {
            refreshProject(context);
            kick();
        })
    );
    kick();
}

function deactivate() {
    return stopClient();
}

module.exports = { activate, deactivate, startClient, stopClient, refreshProject, showProject, parseToml };
