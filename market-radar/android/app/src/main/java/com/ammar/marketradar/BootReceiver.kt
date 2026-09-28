package com.ammar.marketradar
import android.content.*
class BootReceiver: BroadcastReceiver(){ override fun onReceive(c:Context,i:Intent){ c.startForegroundService(Intent(c,RadarService::class.java)) } }
