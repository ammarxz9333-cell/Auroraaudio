import 'dart:convert';
import 'dart:io';

import 'package:cached_network_image/cached_network_image.dart';
import 'package:flutter/material.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:video_player/video_player.dart';
import 'package:webview_flutter/webview_flutter.dart';

part 'src/model.dart';
part 'src/app.dart';
part 'src/repositories.dart';
part 'src/storage.dart';
part 'src/pages.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const PixVaultApp());
}
