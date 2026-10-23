import 'package:dakit_api/dakit_api.dart';
import 'package:dakit_core/dakit_core.dart';

import '../diagnostics/app_logger.dart';
import 'proxy_controller.dart';

/// The OS can advertise an unusable HTTP proxy on Android while Chrome still
/// has network access. A *network-only* failure on the documented DeviantArt
/// HTTPS token endpoint can be retried without that automatically detected
/// system proxy. Never ignore a user-selected proxy and never retry a received
/// HTTP response, invalid_grant, or an uncertain receive/send timeout.
bool mayRetryOAuthDirect({
  required ProxySource source,
  required Uri endpoint,
  required Object error,
}) {
  if (source != ProxySource.system ||
      endpoint.scheme != 'https' ||
      endpoint.host != 'www.deviantart.com' ||
      endpoint.path != '/oauth2/token' ||
      error is! DAKitException ||
      error.kind != DAKitFailureKind.network) {
    return false;
  }
  return error.code == 'network.connection' ||
      error.code == 'network.connect_timeout';
}

/// Does not log URLs, authorization codes, verifier values or token bodies.
final class SafeOAuthNetworkFallback implements OAuthEndpoint {
  const SafeOAuthNetworkFallback({
    required this.primary,
    required this.direct,
    required this.source,
  });

  final OAuthEndpoint primary;
  final OAuthEndpoint direct;
  final ProxySource Function() source;

  @override
  Future<Map<String, Object?>> postForm(
    Uri endpoint,
    Map<String, String> form,
  ) async {
    try {
      return await primary.postForm(endpoint, form);
    } on Object catch (error, stack) {
      if (!mayRetryOAuthDirect(
        source: source(),
        endpoint: endpoint,
        error: error,
      )) {
        Error.throwWithStackTrace(error, stack);
      }
      AppLogger.instance.warning(
        'oauth',
        'Automatic system proxy unreachable; trying direct HTTPS OAuth once',
      );
      try {
        return await direct.postForm(endpoint, form);
      } on Object {
        Error.throwWithStackTrace(error, stack);
      }
    }
  }
}
