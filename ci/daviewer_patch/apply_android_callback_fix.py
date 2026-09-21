from pathlib import Path

p = Path("DAViewer/android/app/src/main/AndroidManifest.xml")
s = p.read_text()
anchor = '''            <meta-data
              android:name="io.flutter.embedding.android.NormalTheme"
              android:resource="@style/NormalTheme"
              />'''
insert = '''            <meta-data
              android:name="io.flutter.embedding.android.NormalTheme"
              android:resource="@style/NormalTheme"
              />
            <!-- app_links owns OAuth deep-link delivery. Flutter's default
                 deep-link handler must be disabled or both handlers compete
                 for dakit://oauth/callback on warm/cold starts. -->
            <meta-data
              android:name="flutter_deeplinking_enabled"
              android:value="false" />'''
if anchor not in s:
    raise SystemExit("NormalTheme metadata anchor changed")
s = s.replace(anchor, insert, 1)
p.write_text(s)
