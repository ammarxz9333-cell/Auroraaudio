package com.ammar.marketradar
import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

object AlertStore {
 fun add(c:Context,o:JSONObject){ val p=c.getSharedPreferences("radar",Context.MODE_PRIVATE);val a=try{JSONArray(p.getString("alerts","[]"))}catch(_:Exception){JSONArray()}; val out=JSONArray();out.put(o);for(i in 0 until minOf(a.length(),99))out.put(a.get(i));p.edit().putString("alerts",out.toString()).apply() }
 fun text(c:Context):String{ val p=c.getSharedPreferences("radar",Context.MODE_PRIVATE);val a=try{JSONArray(p.getString("alerts","[]"))}catch(_:Exception){JSONArray()};if(a.length()==0)return "لا توجد تنبيهات بعد.";return buildString{for(i in 0 until minOf(a.length(),30)){val x=a.getJSONObject(i);append(x.optString("class")).append("  ").append(x.optString("ticker","—")).append("\n").append(x.optString("title").take(120)).append("\n").append(x.optString("market")).append("\n\n")}}}
}
