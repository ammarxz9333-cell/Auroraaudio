package com.ammar.marketradar
import android.Manifest
import android.content.*
import android.os.*
import android.provider.Settings
import android.widget.*
import androidx.appcompat.app.AppCompatActivity

class MainActivity: AppCompatActivity() {
 override fun onCreate(b: Bundle?) { super.onCreate(b)
  if (Build.VERSION.SDK_INT >= 33) requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7)
  startForegroundService(Intent(this, RadarService::class.java))
  val root=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL; setPadding(36,48,36,24) }
  root.addView(TextView(this).apply { text="MARKET RADAR"; textSize=28f })
  root.addView(TextView(this).apply { text="● المراقبة تعمل\nيفحص المصادر العامة باستمرار ويعطي تنبيهًا مبكرًا عند ظهور catalyst مهم.\n\n🚨 HIGH-CONVICTION EARLY\n🟡 DEVELOPING\n⚪ لا تنبيه إذا لم توجد فرصة واضحة"; textSize=18f; setPadding(0,28,0,28) })
  root.addView(Button(this).apply { text="استثناء التطبيق من توفير البطارية"; setOnClickListener { startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) } })
  root.addView(Button(this).apply { text="تشغيل الرادار الآن"; setOnClickListener { startForegroundService(Intent(this@MainActivity,RadarService::class.java).putExtra("scan_now",true)) } })
  setContentView(root)
 }
}
