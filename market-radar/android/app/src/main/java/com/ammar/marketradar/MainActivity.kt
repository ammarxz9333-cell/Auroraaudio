package com.ammar.marketradar
import android.Manifest
import android.content.*
import android.os.*
import android.provider.Settings
import android.widget.*
import androidx.appcompat.app.AppCompatActivity

class MainActivity: AppCompatActivity() {
 private lateinit var history:TextView
 override fun onCreate(b:Bundle?){super.onCreate(b)
  if(Build.VERSION.SDK_INT>=33)requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS),7)
  startForegroundService(Intent(this,RadarService::class.java))
  val root=LinearLayout(this).apply{orientation=LinearLayout.VERTICAL;setPadding(36,48,36,24)}
  root.addView(TextView(this).apply{text="MARKET RADAR";textSize=28f})
  root.addView(TextView(this).apply{text="● المراقبة تعمل على الهاتف\nForeground radar + premarket early gate + local alerts";textSize=18f;setPadding(0,20,0,20)})
  root.addView(Button(this).apply{text="استثناء التطبيق من توفير البطارية";setOnClickListener{startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))}})
  root.addView(Button(this).apply{text="فحص الآن";setOnClickListener{startForegroundService(Intent(this@MainActivity,RadarService::class.java).putExtra("scan_now",true))}})
  root.addView(TextView(this).apply{text="آخر التنبيهات";textSize=21f;setPadding(0,30,0,12)})
  history=TextView(this).apply{textSize=15f}
  val scroll=ScrollView(this);scroll.addView(history);root.addView(scroll,LinearLayout.LayoutParams(-1,0,1f))
  setContentView(root)
 }
 override fun onResume(){super.onResume();history.text=AlertStore.text(this)}
}
