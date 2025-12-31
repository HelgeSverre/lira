import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
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
