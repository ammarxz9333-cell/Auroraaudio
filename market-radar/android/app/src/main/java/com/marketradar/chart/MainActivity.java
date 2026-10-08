package com.marketradar.chart;

import android.app.Activity;
import android.os.Bundle;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Path;
import android.view.View;
import android.view.Gravity;
import android.widget.*;
import android.content.Context;
import android.graphics.Typeface;
import org.json.JSONArray;
import org.json.JSONObject;
import java.net.URL;
import java.net.HttpURLConnection;
import java.io.InputStream;
import java.io.ByteArrayOutputStream;
import java.util.ArrayList;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends Activity {
  private static final int BG=Color.rgb(9,17,31), PANEL=Color.rgb(17,29,48), WHITE=Color.rgb(235,242,255);
  private final ExecutorService executor=Executors.newSingleThreadExecutor();
  private final ArrayList<Candle> candles=new ArrayList<>();
  private String symbol="CRWV", interval="1d", range="6mo";
  private TextView status,price,analysis;
  private Chart chart;
  private int dp(float n){return (int)(getResources().getDisplayMetrics().density*n+.5f);}
  private TextView text(String s,int size,int color){TextView v=new TextView(this);v.setText(s);v.setTextSize(size);v.setTextColor(color);v.setPadding(dp(8),dp(8),dp(8),dp(8));return v;}
  private static final class Candle {
    long time; float o,h,l,c;
    Candle(long t,float a,float b,float d,float e){time=t;o=a;h=b;l=d;c=e;}
  }
  @Override public void onCreate(Bundle b){
    super.onCreate(b);
    getWindow().setStatusBarColor(BG);getWindow().setNavigationBarColor(BG);
    LinearLayout root=new LinearLayout(this);root.setOrientation(LinearLayout.VERTICAL);root.setBackgroundColor(BG);
    ScrollView scroll=new ScrollView(this);scroll.setFillViewport(true);scroll.addView(root);
    TextView title=text("MARKET RADAR  •  تحليل الأسهم",23,WHITE);title.setTypeface(null,Typeface.BOLD);root.addView(title);
    TextView sub=text("تطبيق أندرويد أصلي • شموع سعرية حقيقية • بدون بيانات تجريبية",12,0xff91a7c5);root.addView(sub);
    Spinner stocks=new Spinner(this);String[] names={"CRWV","APLD","SOFI","NVDA","QQQ","SPY","GLD","AMD","TSLA","PLTR","GOOG","AAPL"};
    ArrayAdapter<String> adapter=new ArrayAdapter<>(this,android.R.layout.simple_spinner_dropdown_item,names);stocks.setAdapter(adapter);
    root.addView(text("السهم",13,WHITE));root.addView(stocks);
    LinearLayout timeRow=new LinearLayout(this);timeRow.setOrientation(LinearLayout.HORIZONTAL);
    String[] periods={"5m","15m","1h","1d","1wk"};
    for(String p:periods){Button btn=new Button(this);btn.setText(p);btn.setTextSize(12);timeRow.addView(btn,new LinearLayout.LayoutParams(0,dp(52),1));btn.setOnClickListener(v->{interval=p;range=p.equals("5m")||p.equals("15m")?"5d":p.equals("1h")?"1mo":p.equals("1wk")?"2y":"6mo";load();});}
    root.addView(timeRow);
    price=text("—",27,WHITE);price.setTypeface(null,Typeface.BOLD);root.addView(price);
    status=text("جارٍ تحميل بيانات السوق...",13,0xff91a7c5);root.addView(status);
    chart=new Chart(this);root.addView(chart,new LinearLayout.LayoutParams(-1,dp(390)));
    analysis=text("التحليل الفني سيظهر بعد تحميل الشموع.",15,WHITE);analysis.setBackgroundColor(PANEL);root.addView(analysis,new LinearLayout.LayoutParams(-1,-2));
    Button refresh=new Button(this);refresh.setText("تحديث الأسعار والتحليل");root.addView(refresh);refresh.setOnClickListener(v->load());
    root.addView(text("مصدر الأسعار: Yahoo Finance chart API غير الرسمي. الأسعار قد تتأخر. إشارات الدعم والمقاومة تقديرية وليست توصية مالية.",12,0xff91a7c5));
    setContentView(scroll);
    stocks.setOnItemSelectedListener(new android.widget.AdapterView.OnItemSelectedListener(){
      public void onNothingSelected(android.widget.AdapterView<?> parent){}
      public void onItemSelected(android.widget.AdapterView<?> parent,View v,int pos,long id){symbol=names[pos];load();}
    });
  }
  private static byte[] read(InputStream stream)throws Exception{
    ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] b=new byte[8192];int n;
    while((n=stream.read(b))!=-1)out.write(b,0,n);
    return out.toByteArray();
  }
  private void load(){
    final String s=symbol,p=interval,r=range;
    status.setText("جارٍ تحميل "+s+" ("+p+")...");
    executor.execute(()->{
      try{
        String url="https://query1.finance.yahoo.com/v8/finance/chart/"+s+"?interval="+p+"&range="+r;
        HttpURLConnection con=(HttpURLConnection)new URL(url).openConnection();
        con.setConnectTimeout(12000);con.setReadTimeout(12000);con.setRequestProperty("User-Agent","Mozilla/5.0 MarketRadar/1.0");
        int code=con.getResponseCode();if(code!=200)throw new Exception("HTTP "+code+" من مصدر البيانات");
        String body;try(InputStream in=con.getInputStream()){body=new String(read(in),java.nio.charset.StandardCharsets.UTF_8);}finally{con.disconnect();}
        JSONObject result=new JSONObject(body).getJSONObject("chart").getJSONArray("result").getJSONObject(0);
        JSONArray times=result.getJSONArray("timestamp");
        JSONObject quote=result.getJSONObject("indicators").getJSONArray("quote").getJSONObject(0);
        JSONArray os=quote.getJSONArray("open"),hs=quote.getJSONArray("high"),ls=quote.getJSONArray("low"),cs=quote.getJSONArray("close");
        ArrayList<Candle> next=new ArrayList<>();
        for(int i=0;i<times.length();i++){
          if(os.isNull(i)||hs.isNull(i)||ls.isNull(i)||cs.isNull(i))continue;
          double o=os.getDouble(i),h=hs.getDouble(i),l=ls.getDouble(i),c=cs.getDouble(i);
          if(!Double.isFinite(o)||!Double.isFinite(h)||!Double.isFinite(l)||!Double.isFinite(c)||l>h)continue;
          next.add(new Candle(times.getLong(i),(float)o,(float)h,(float)l,(float)c));
        }
        if(next.size()<12)throw new Exception("لا توجد شموع كافية للتحليل");
        runOnUiThread(()->{
          if(!s.equals(symbol)||!p.equals(interval))return;
          candles.clear();candles.addAll(next);
          float last=next.get(next.size()-1).c,first=next.get(0).c;
          price.setText(String.format(Locale.US,"%s  $%.2f  (%+.2f%%)",s,last,100*(last-first)/first));
          status.setText("✓ "+next.size()+" شمعة فعلية • "+p+" • آخر شمعة "+new java.text.SimpleDateFormat("yyyy-MM-dd HH:mm",Locale.US).format(new java.util.Date(next.get(next.size()-1).time*1000L)));
          analysis.setText(analyse(next));chart.invalidate();
        });
      }catch(Exception e){runOnUiThread(()->{if(s.equals(symbol)&&p.equals(interval)){candles.clear();chart.invalidate();price.setText("—");status.setText("تعذّر جلب الأسعار: "+e.getMessage());analysis.setText("لا نعرض أسعارًا أو إشارات وهمية عند فشل مصدر البيانات.");}});}
    });
  }
  private String analyse(ArrayList<Candle> a){
    int n=a.size();float last=a.get(n-1).c;
    ArrayList<Float> highs=new ArrayList<>(),lows=new ArrayList<>();
    for(int i=3;i<n-3;i++){
      boolean hi=true,lo=true;
      for(int j=i-3;j<=i+3;j++)if(j!=i){if(a.get(j).h>=a.get(i).h)hi=false;if(a.get(j).l<=a.get(i).l)lo=false;}
      if(hi)highs.add(a.get(i).h);if(lo)lows.add(a.get(i).l);
    }
    float support=Float.NaN,resistance=Float.NaN;
    for(float v:lows)if(v<last&&(Float.isNaN(support)||v>support))support=v;
    for(float v:highs)if(v>last&&(Float.isNaN(resistance)||v<resistance))resistance=v;
    float sma20=0;for(int i=Math.max(0,n-20);i<n;i++)sma20+=a.get(i).c;sma20/=Math.min(n,20);
    String trend=last>sma20?"أعلى من المتوسط 20 — ميل إيجابي":last<sma20?"أقل من المتوسط 20 — ميل سلبي":"قريب من المتوسط 20";
    return "الاتجاه: "+trend+"\nالمتوسط SMA20: $"+String.format(Locale.US,"%.2f",sma20)
      +"\nالدعم المحوري: "+(Float.isNaN(support)?"غير محدد":String.format(Locale.US,"$%.2f",support))
      +"\nالمقاومة المحورية: "+(Float.isNaN(resistance)?"غير محددة":String.format(Locale.US,"$%.2f",resistance))
      +"\nالاختراق يحتاج إغلاقًا مؤكّدًا فوق المقاومة؛ كسر الدعم يضعف السيناريو الصاعد. التحليل آلي وتقريبي.";
  }
  private final class Chart extends View{
    private final Paint paint=new Paint(3);
    Chart(Context c){super(c);setBackgroundColor(PANEL);}
    private void line(Canvas c,int color,float width,float x,float y,float xx,float yy){paint.setColor(color);paint.setStrokeWidth(width);paint.setStyle(Paint.Style.STROKE);c.drawLine(x,y,xx,yy,paint);}
    @Override protected void onDraw(Canvas canvas){
      super.onDraw(canvas);
      if(candles.isEmpty()){paint.setColor(WHITE);paint.setTextSize(dp(16));canvas.drawText("بانتظار بيانات حقيقية...",dp(18),getHeight()/2f,paint);return;}
      int n=candles.size(),start=Math.max(0,n-90),count=n-start;
      float min=Float.MAX_VALUE,max=-Float.MAX_VALUE;
      for(int i=start;i<n;i++){min=Math.min(min,candles.get(i).l);max=Math.max(max,candles.get(i).h);}
      float margin=Math.max(.01f,(max-min)*.08f);min-=margin;max+=margin;
      float left=dp(12),right=getWidth()-dp(12),top=dp(22),bottom=getHeight()-dp(28),span=max-min;
      for(int i=0;i<=4;i++){float y=top+(bottom-top)*i/4f;line(canvas,0xff30405a,1,left,y,right,y);paint.setColor(0xff9aacc6);paint.setTextSize(dp(10));canvas.drawText(String.format(Locale.US,"%.2f",max-span*i/4f),left+dp(3),y-dp(3),paint);}
      float step=(right-left)/count,w=Math.max(1,step*.55f);
      for(int i=start;i<n;i++){
        Candle k=candles.get(i);float x=left+(i-start+.5f)*step;
        int color=k.c>=k.o?0xff2dd4a1:0xfffb7185;
        float yh=top+(max-k.h)/span*(bottom-top),yl=top+(max-k.l)/span*(bottom-top),yo=top+(max-k.o)/span*(bottom-top),yc=top+(max-k.c)/span*(bottom-top);
        line(canvas,color,dp(1),x,yh,x,yl);
        paint.setColor(color);paint.setStyle(Paint.Style.FILL);canvas.drawRect(x-w/2,Math.min(yo,yc),x+w/2,Math.max(Math.max(yo,yc),Math.min(yo,yc)+dp(1)),paint);
      }
      if(n>=20){Path path=new Path();for(int i=Math.max(start,19);i<n;i++){float sum=0;for(int j=i-19;j<=i;j++)sum+=candles.get(j).c;float y=top+(max-sum/20)/span*(bottom-top),x=left+(i-start+.5f)*step;if(i==Math.max(start,19))path.moveTo(x,y);else path.lineTo(x,y);}paint.setColor(0xfffbbf24);paint.setStrokeWidth(dp(2));paint.setStyle(Paint.Style.STROKE);canvas.drawPath(path,paint);paint.setStyle(Paint.Style.FILL);}
    }
  }
  @Override protected void onDestroy(){executor.shutdownNow();super.onDestroy();}
}
