import 'package:dio/dio.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:daviewer/features/multisource/gallery_catalog.dart';
import 'package:daviewer/features/multisource/multisource_screen.dart';

void main() {
  testWidgets('MultiSource auto-loads four providers without search typing',
      (tester) async {
    final paths = <String>[];
    final dio = Dio();
    dio.interceptors.add(InterceptorsWrapper(
      onRequest: (request, handler) {
        paths.add(request.uri.host);
        final data = switch (request.uri.host) {
          'danbooru.donmai.us' => <dynamic>[],
          'safebooru.org' => <dynamic>[],
          'wallhaven.cc' => <String, dynamic>{
              'data': <dynamic>[],
              'meta': {'last_page': 1},
            },
          'api.openverse.org' => <String, dynamic>{
              'results': <dynamic>[],
              'page_count': 1,
            },
          _ => <dynamic>[],
        };
        handler.resolve(Response<dynamic>(
          requestOptions: request,
          statusCode: 200,
          data: data,
        ));
      },
    ));

    await tester.pumpWidget(MaterialApp(
      home: MultiSourceScreen(catalog: GalleryCatalog(dio: dio)),
    ));
    await tester.pumpAndSettle();

    expect(
      paths.toSet(),
      <String>{
        'danbooru.donmai.us',
        'safebooru.org',
        'wallhaven.cc',
        'api.openverse.org',
      },
    );
    expect(paths.length, 4);
    final field = tester.widget<TextField>(find.byType(TextField).first);
    expect(field.controller?.text, 'landscape');
    expect(find.text('No images here yet. Try Landscape, Anime, or another source.'),
        findsOneWidget);
  });
}
