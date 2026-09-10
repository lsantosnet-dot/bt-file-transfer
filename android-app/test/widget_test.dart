// Teste de smoke: garante que o app sobe sem lançar exceções e mostra
// o botão de envio desabilitado antes de qualquer dispositivo/arquivo
// ser selecionado.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:android_app/main.dart';

void main() {
  testWidgets('App inicia mostrando o botao de envio desabilitado', (WidgetTester tester) async {
    await tester.pumpWidget(const BtFileTransferApp());
    await tester.pump();

    expect(find.text('Enviar via Bluetooth'), findsOneWidget);

    final button = tester.widget<FilledButton>(find.byWidgetPredicate((w) => w is FilledButton));
    expect(button.onPressed, isNull);
  });
}
