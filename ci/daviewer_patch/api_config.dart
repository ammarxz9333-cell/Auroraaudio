import 'package:dakit_core/dakit_core.dart';

final class ApiConfig {
  ApiConfig({
    Uri? baseUri,
    this.minorVersion = 20240701,
    this.userAgent = 'DAKit/0.1',
    this.connectTimeout = const Duration(seconds: 15),
    this.receiveTimeout = const Duration(seconds: 30),
    this.retryPolicy = const RetryPolicy(),
    this.rateLimitPolicy = const RateLimitPolicy(),
  }) : baseUri = baseUri ?? Uri.https('www.deviantart.com', '/api/v1/oauth2/') {
    if (!this.baseUri.isScheme('https')) {
      throw const DAKitException(
        kind: DAKitFailureKind.configuration,
        code: 'api.base_uri.insecure',
        message: 'The official API base URI must use HTTPS.',
      );
    }
    if (minorVersion <= 0 || userAgent.trim().isEmpty) {
      throw const DAKitException(
        kind: DAKitFailureKind.configuration,
        code: 'api.config.invalid',
        message: 'API minor version and User-Agent must be configured.',
      );
    }
  }

  final Uri baseUri;
  final int minorVersion;
  final String userAgent;
  final Duration connectTimeout;
  final Duration receiveTimeout;
  final RetryPolicy retryPolicy;
  final RateLimitPolicy rateLimitPolicy;
}

final class RetryPolicy {
  const RetryPolicy({
    this.maxRetries = 3,
    this.initialDelay = const Duration(seconds: 1),
    this.maximumDelay = const Duration(seconds: 8),
  }) : assert(maxRetries >= 0);

  final int maxRetries;
  final Duration initialDelay;
  final Duration maximumDelay;

  Duration delayFor(int retry) {
    final multiplier = 1 << (retry - 1).clamp(0, 30);
    final milliseconds = initialDelay.inMilliseconds * multiplier;
    return Duration(
      milliseconds: milliseconds.clamp(0, maximumDelay.inMilliseconds),
    );
  }
}

/// Conservative client-side pacing for DeviantArt's undocumented dynamic
/// OAuth rate limits. This does not bypass provider limits; it prevents bursts,
/// honours Retry-After, and keeps idempotent reads alive across long cooldowns.
final class RateLimitPolicy {
  const RateLimitPolicy({
    this.maxRetries = 10,
    this.initialDelay = const Duration(seconds: 4),
    this.maximumDelay = const Duration(minutes: 5),
    this.minimumSpacing = const Duration(milliseconds: 350),
  }) : assert(maxRetries >= 0);

  final int maxRetries;
  final Duration initialDelay;
  final Duration maximumDelay;
  final Duration minimumSpacing;

  Duration delayFor(int retry) {
    final multiplier = 1 << (retry - 1).clamp(0, 30);
    final milliseconds = initialDelay.inMilliseconds * multiplier;
    return Duration(
      milliseconds: milliseconds.clamp(0, maximumDelay.inMilliseconds),
    );
  }
}
