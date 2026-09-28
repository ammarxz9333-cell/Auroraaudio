package com.ammar.marketradar
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.time.*
import kotlin.math.abs

data class MarketSnapshot(val ticker:String,val price:Double,val previousClose:Double,val changePct:Double,val change5mPct:Double?,val rvol:Double?,val vwap:Double?,val holdsVwap:Boolean?,val session:String,val reaction:String,val barTime:Long)
data class EarlyDecision(val state:String,val reason:String)

object MarketEngine {
 private const val UA="Ammar-Market-Radar/1.0"
 fun snapshot(ticker:String):MarketSnapshot? = try {
  val u="https://query1.finance.yahoo.com/v8/finance/chart/"+ticker+"?interval=5m&range=5d&includePrePost=true&events=div%2Csplits"
  val c=URL(u).openConnection() as HttpURLConnection; c.connectTimeout=9000;c.readTimeout=9000;c.setRequestProperty("User-Agent",UA)
  val root=JSONObject(c.inputStream.bufferedReader().readText()).getJSONObject("chart").getJSONArray("result").getJSONObject(0)
  val meta=root.getJSONObject("meta"); val ts=root.getJSONArray("timestamp"); val q=root.getJSONObject("indicators").getJSONArray("quote").getJSONObject(0)
  data class B(val t:Long,val p:Double,val h:Double,val l:Double,val v:Long,val ny:ZonedDateTime)
  val bars=mutableListOf<B>(); val zone=ZoneId.of("America/New_York")
  for(i in 0 until ts.length()){ if(q.getJSONArray("close").isNull(i))continue; val t=ts.getLong(i);val p=q.getJSONArray("close").getDouble(i); val h=if(q.getJSONArray("high").isNull(i))p else q.getJSONArray("high").getDouble(i);val l=if(q.getJSONArray("low").isNull(i))p else q.getJSONArray("low").getDouble(i);val v=if(q.getJSONArray("volume").isNull(i))0 else q.getJSONArray("volume").getLong(i);bars+=B(t,p,h,l,v,Instant.ofEpochSecond(t).atZone(zone))}
  if(bars.isEmpty()) return null
  val last=bars.last(); val prev=meta.optDouble("regularMarketPreviousClose",meta.optDouble("chartPreviousClose",bars.first().p));val mins=last.ny.hour*60+last.ny.minute
  val session=if(mins<570)"PRE" else if(mins<960)"REGULAR" else "AFTER/CLOSED"
  val today=last.ny.toLocalDate(); val cutoff=if(session=="PRE")570 else minOf(mins,959)
  fun regular(b:B)=b.ny.hour*60+b.ny.minute in 570..959
  val todayReg=bars.filter{it.ny.toLocalDate()==today&&regular(it)&&it.ny.hour*60+it.ny.minute<=cutoff}
  val vden=todayReg.sumOf{it.v}; val vwap=if(vden>0)todayReg.sumOf{((it.h+it.l+it.p)/3.0)*it.v}/vden else null
  val hist=bars.filter{it.ny.toLocalDate()!=today&&regular(it)}.groupBy{it.ny.toLocalDate()}.values.map{day->day.filter{it.ny.hour*60+it.ny.minute<=cutoff}.sumOf{it.v}}.filter{it>0}.sorted()
  val med=if(hist.isEmpty())null else if(hist.size%2==1)hist[hist.size/2].toDouble() else (hist[hist.size/2-1]+hist[hist.size/2])/2.0
  val rvol=if(vden>0&&med!=null&&med>0)vden/med else null; val change=(last.p/prev-1)*100;val c5=if(bars.size>1)(last.p/bars[bars.size-2].p-1)*100 else null
  val reaction=if(abs(change)>=10||(rvol?:0.0)>=4)"major-reprice" else if(abs(change)>=3||(rvol?:0.0)>=2)"reacting" else if(abs(change)<2&&(rvol==null||rvol<1.25))"not-yet-reacted" else "mixed/early"
  MarketSnapshot(ticker,last.p,prev,change,c5,rvol,vwap,vwap?.let{last.p>=it},session,reaction,last.t)
 } catch(_:Exception){null}

 fun early(score:Int,s:MarketSnapshot,dilution:Boolean=false,scheduledBinary:Boolean=false):EarlyDecision {
  if(s.session!="PRE") return EarlyDecision("NO_EARLY","premarket only")
  if(dilution) return EarlyDecision("WATCH","dilution/financing risk")
  if(abs(s.changePct)>=25) return EarlyDecision("LATE","already repriced >=25%")
  val rv=s.rvol?:0.0
  if(scheduledBinary&&rv>=1.5&&abs(s.changePct)<15) return EarlyDecision("EARLY_DEVELOPING","scheduled binary catalyst + accumulation")
  if(score>=10&&rv>=2&&abs(s.changePct)<15) return EarlyDecision("HIGH_CONVICTION_EARLY","strong catalyst + abnormal participation before large reprice")
  if(score>=8&&(rv>=1.5||(s.change5mPct?:0.0)>=1.5)&&abs(s.changePct)<20) return EarlyDecision("EARLY_DEVELOPING","fresh catalyst + accelerating premarket tape")
  return EarlyDecision("WATCH","not enough premarket confirmation")
 }
}
