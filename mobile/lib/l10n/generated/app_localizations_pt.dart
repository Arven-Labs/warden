// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Portuguese (`pt`).
class AppLocalizationsPt extends AppLocalizations {
  AppLocalizationsPt([String locale = 'pt']) : super(locale);

  @override
  String get languageLabel => 'Idioma';

  @override
  String get languageEnglish => 'English';

  @override
  String get languagePortuguese => 'Português (Brasil)';

  @override
  String get languageSystem => 'Seguir o sistema';

  @override
  String get commonCancel => 'Cancelar';

  @override
  String get commonSave => 'Salvar';

  @override
  String get commonEdit => 'Editar';

  @override
  String get commonRemove => 'Remover';

  @override
  String get commonDelete => 'Apagar';

  @override
  String get commonKeep => 'Manter';

  @override
  String get commonClose => 'Fechar';

  @override
  String get commonRefresh => 'Atualizar';

  @override
  String get commonLoading => 'Carregando…';

  @override
  String get commonCopy => 'Copiar';

  @override
  String get commonDone => 'Pronto';

  @override
  String get commonTryAgain => 'Tentar de novo';

  @override
  String get folderTitle => 'Pasta de trabalho';

  @override
  String get folderHint =>
      'A IA lê e escreve nesta pasta, e o shell começa nela (cada comando pede o seu sim). Vale para a conversa inteira e não muda depois.';

  @override
  String get folderThisHub => 'Este hub';

  @override
  String get folderYours => 'Suas pastas';

  @override
  String get folderUp => 'Acima';

  @override
  String get folderEmpty => 'Sem subpastas.';

  @override
  String get folderUse => 'Usar esta pasta';

  @override
  String get connTitle => 'Warden — Conexão com o servidor';

  @override
  String get connDiscoverTooltip => 'Procurar hubs na rede local';

  @override
  String get connScanTooltip => 'Escanear QR para preencher a conexão';

  @override
  String get connAccountTooltip => 'Minha conta';

  @override
  String get connSyncTooltip => 'Sync';

  @override
  String get connSkillsTooltip => 'Skills';

  @override
  String get connHost => 'Endereço do servidor';

  @override
  String get connPort => 'Porta';

  @override
  String get connUseTls => 'Usar TLS (wss://)';

  @override
  String get connUseTlsHint =>
      'Para um hub que serve HTTPS — use o nome que o certificado cobre, ex.: hub.tailXXXX.ts.net';

  @override
  String get connPairingKey => 'Chave de pareamento';

  @override
  String get connUsername => 'Usuário';

  @override
  String get connPassword => 'Senha';

  @override
  String get connPasswordHint => 'Só é preciso na primeira vez neste celular';

  @override
  String get connAuthKey => 'Chave de autenticação';

  @override
  String get connAuthKeyHint => 'Chave de pareamento, ou um código de convite';

  @override
  String get connDeviceName => 'Nome do dispositivo';

  @override
  String get connResume => 'Voltar à conversa';

  @override
  String get connDisconnect => 'Desconectar';

  @override
  String get connConnect => 'Conectar';

  @override
  String get connFillAccount =>
      'Preencha endereço, porta, usuário, senha e nome do dispositivo';

  @override
  String get connFillKey =>
      'Preencha endereço, porta, chave de autenticação e nome do dispositivo';

  @override
  String get connLocked =>
      'Seus dados estão trancados porque o hub reiniciou. Entre uma vez com usuário e senha para destrancá-los.';

  @override
  String get connChoosePassword => 'Escolha a sua própria senha para continuar';

  @override
  String connDiscoveryFailed(String error) {
    return 'A busca falhou: $error';
  }

  @override
  String get connNoHub => 'Nenhum hub respondeu na rede local.';

  @override
  String get connHubsFound => 'Hubs encontrados';

  @override
  String get connStatusDisconnected => 'Desconectado';

  @override
  String connStatusDisconnectedWhy(String reason) {
    return 'Desconectado: $reason';
  }

  @override
  String get connStatusConnecting => 'Conectando…';

  @override
  String connStatusConnected(String server) {
    return 'Conectado a $server';
  }

  @override
  String connStatusError(String message) {
    return 'Erro: $message';
  }
}
