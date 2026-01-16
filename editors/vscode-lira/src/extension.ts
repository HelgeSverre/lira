import * as vscode from 'vscode';
import * as cp from 'child_process';
import * as path from 'path';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let outputChannel: vscode.OutputChannel;

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('Lira');

    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('lira.runFile', runFile),
        vscode.commands.registerCommand('lira.checkFile', checkFile),
        vscode.commands.registerCommand('lira.showAST', showAST)
    );

    const config = vscode.workspace.getConfiguration('lira');
    const enableLsp = config.get<boolean>('languageServer.enable', true);

    if (!enableLsp) {
        console.log('Lira LSP is disabled');
        return;
    }

    const serverPath = config.get<string>('languageServer.path', 'lira-lsp');

    const serverOptions: ServerOptions = {
        command: serverPath,
        args: [],
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'lira' }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.{li,lira}'),
        },
    };

    client = new LanguageClient(
        'liraLanguageServer',
        'Lira Language Server',
        serverOptions,
        clientOptions
    );

    client.start();
    console.log('Lira language server started');
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}

async function runFile() {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'lira') {
        vscode.window.showErrorMessage('No Lira file open');
        return;
    }

    await editor.document.save();
    const filePath = editor.document.fileName;

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine(`Running: ${path.basename(filePath)}\n`);

    const process = cp.spawn('lira', ['run', filePath]);

    process.stdout.on('data', (data) => outputChannel.append(data.toString()));
    process.stderr.on('data', (data) => outputChannel.append(data.toString()));
    process.on('close', (code) => {
        outputChannel.appendLine(`\nProcess exited with code ${code}`);
    });
    process.on('error', (err) => {
        outputChannel.appendLine(`\nError: ${err.message}`);
        vscode.window.showErrorMessage(`Failed to run lira: ${err.message}`);
    });
}

async function checkFile() {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'lira') {
        vscode.window.showErrorMessage('No Lira file open');
        return;
    }

    await editor.document.save();
    const filePath = editor.document.fileName;

    outputChannel.clear();
    outputChannel.show();
    outputChannel.appendLine(`Checking: ${path.basename(filePath)}\n`);

    cp.exec(`lira check "${filePath}"`, (error, stdout, stderr) => {
        if (stdout) outputChannel.append(stdout);
        if (stderr) outputChannel.append(stderr);
        if (!error) {
            outputChannel.appendLine('No errors found!');
        }
    });
}

async function showAST() {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'lira') {
        vscode.window.showErrorMessage('No Lira file open');
        return;
    }

    await editor.document.save();
    const filePath = editor.document.fileName;

    cp.exec(`lira ast "${filePath}"`, async (error, stdout, stderr) => {
        if (error) {
            vscode.window.showErrorMessage(`AST error: ${stderr}`);
            return;
        }

        const doc = await vscode.workspace.openTextDocument({
            content: stdout,
            language: 'json'
        });
        await vscode.window.showTextDocument(doc, { viewColumn: vscode.ViewColumn.Beside });
    });
}
