part of '../main.dart';

class PixVaultApp extends StatefulWidget {
  const PixVaultApp({super.key});

  @override
  State<PixVaultApp> createState() => _PixVaultAppState();
}

class _PixVaultAppState extends State<PixVaultApp> {
  bool? accepted;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final p = await SharedPreferences.getInstance();
    if (mounted) setState(() => accepted = p.getBool('ageAccepted') ?? false);
  }

  @override
  Widget build(BuildContext context) {
    final scheme = ColorScheme.fromSeed(
      seedColor: const Color(0xff00c8a7),
      brightness: Brightness.dark,
    );
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'PixVault',
      theme: ThemeData(
        colorScheme: scheme,
        brightness: Brightness.dark,
        scaffoldBackgroundColor: const Color(0xff0c0f12),
        cardColor: const Color(0xff14191e),
        useMaterial3: true,
      ),
      home: accepted == null
          ? const Scaffold(body: Center(child: CircularProgressIndicator()))
          : accepted!
              ? const HomeShell()
              : AgeGate(onAccepted: () => setState(() => accepted = true)),
    );
  }
}

class AgeGate extends StatelessWidget {
  final VoidCallback onAccepted;
  const AgeGate({super.key, required this.onAccepted});

  Future<void> _accept() async {
    final p = await SharedPreferences.getInstance();
    await p.setBool('ageAccepted', true);
    onAccepted();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Padding(
              padding: const EdgeInsets.all(28),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Icon(Icons.visibility_rounded, size: 74),
                  const SizedBox(height: 20),
                  Text('PixVault', style: Theme.of(context).textTheme.headlineLarge),
                  const SizedBox(height: 14),
                  const Text(
                    'This viewer is for adults only. Verified sources in this build are Rule34Vault/XYZ, yande.re and Pixiv. Searches and tags that explicitly indicate minors are blocked.',
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 28),
                  FilledButton.icon(
                    onPressed: _accept,
                    icon: const Icon(Icons.check_circle_outline),
                    label: const Text('I confirm I am 18 or older'),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
