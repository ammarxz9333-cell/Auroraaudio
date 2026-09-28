package com.ammar.marketradar
import android.app.*
import android.content.*
import android.os.*
import androidx.core.app.NotificationCompat
import java.net.*
import java.security.MessageDigest
import java.time.Instant
import java.util.concurrent.Executors
import javax.xml.parsers.DocumentBuilderFactory

class RadarService: Service() {
 private val exec=Executors.newSingleThreadExecutor()
 private val handler=Handler(Looper.getMainLooper())
 private val interval=120_000L
 private val sources=listOf(
  "SEC 8-K" to "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=8-k&owner=include&count=100&output=atom",
  "SEC S-3" to "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=s-3&owner=include&count=100&output=atom",
  "SEC 424B5" to "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=424b5&owner=include&count=100&output=atom",
  "Google FDA" to "https://news.google.com/rss/search?q=site%3Afda.gov+(approval+OR+PDUFA+OR+clinical+OR+safety)+when%3A1d&hl=en-US&gl=US&ceid=US:en",
  "Google Defense" to "https://news.google.com/rss/search?q=site%3Adefense.gov+(contract+OR+award)+when%3A1d&hl=en-US&gl=US&ceid=US:en",
  "Google M&A" to "https://news.google.com/rss/search?q=(acquisition+OR+merger+OR+takeover+OR+strategic+review)+stocks+when%3A1d&hl=en-US&gl=US&ceid=US:en"
 )
 private val strong=listOf("approval","phase 3","phase iii","contract","award","acquisition","merger","takeover","guidance","buyback","strategic review","pdufa")
 private val risk=listOf("offering","424b5","s-3","dilution","bankruptcy","clinical hold","failed primary")
 override fun onCreate(){ super.onCreate(); channels(); startForeground(1,status("Market Radar يعمل — فحص كل دقيقتين")); schedule(1000) }
 override fun onStartCommand(i:Intent?,f:Int,id:Int):Int { if(i?.getBooleanExtra("scan_now",false)==true) schedule(0); return START_STICKY }
 private fun schedule(delay:Long){ handler.postDelayed({exec.execute{ scan(); schedule(interval) }},delay) }
 private fun scan(){
  val prefs=getSharedPreferences("radar",MODE_PRIVATE)
  sources.forEach { (source,url) -> try {
   val conn=URL(url).openConnection() as HttpURLConnection
   conn.connectTimeout=12000; conn.readTimeout=12000; conn.setRequestProperty("User-Agent","Ammar-Market-Radar/1.0")
   conn.inputStream.use { input ->
    val doc=DocumentBuilderFactory.newInstance().apply{isNamespaceAware=true}.newDocumentBuilder().parse(input)
    val entries=doc.getElementsByTagNameNS("*","entry").let{if(it.length>0)it else doc.getElementsByTagName("item")}
    for(n in 0 until minOf(entries.length,40)){
     val e=entries.item(n); val text=e.textContent.replace("\n"," ").trim(); val low=text.lowercase()
     val id=sha(text); if(prefs.getBoolean(id,false)) continue
     prefs.edit().putBoolean(id,true).apply()
     val positives=strong.count{low.contains(it)}; val negatives=risk.count{low.contains(it)}
     val classification=when { positives>=2 && negatives==0 -> "HIGH-CONVICTION EARLY"; positives>=1 -> "DEVELOPING"; else -> null }
     if(classification!=null) alert(classification,source,text.take(420),negatives>0)
    }
   }
  } catch(_:Exception){} }
 }
 private fun alert(cls:String,source:String,title:String,risky:Boolean){
  val icon=if(cls.startsWith("HIGH")) "🚨" else "🟡"
  val riskText=if(risky)" • ⚠️ financing/risk keyword" else ""
  val n=NotificationCompat.Builder(this,"alerts").setSmallIcon(android.R.drawable.stat_notify_more)
   .setContentTitle("$icon $cls").setContentText(title.take(100)).setStyle(NotificationCompat.BigTextStyle().bigText("$source\n$title$riskText\n\nإشارة مبكرة وليست ضمانًا للارتفاع."))
   .setPriority(NotificationCompat.PRIORITY_MAX).setAutoCancel(true).build()
  (getSystemService(NOTIFICATION_SERVICE) as NotificationManager).notify((System.nanoTime()%Int.MAX_VALUE).toInt(),n)
 }
 private fun status(t:String)=NotificationCompat.Builder(this,"service").setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle("Market Radar").setContentText(t).setOngoing(true).build()
 private fun channels(){ val m=getSystemService(NOTIFICATION_SERVICE) as NotificationManager; m.createNotificationChannel(NotificationChannel("service","Radar service",NotificationManager.IMPORTANCE_LOW)); m.createNotificationChannel(NotificationChannel("alerts","Market alerts",NotificationManager.IMPORTANCE_HIGH)) }
 private fun sha(s:String)=MessageDigest.getInstance("SHA-256").digest(s.toByteArray()).joinToString(""){"%02x".format(it)}
 override fun onBind(i:Intent?)=null
}
