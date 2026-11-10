import numpy as np, math, random, subprocess, sys
from PIL import Image, ImageDraw, ImageFilter
W,H=1280,720; FPS=24; DUR=18.0; S=W/1920
def ld(n,mode): return Image.open(n).convert(mode).resize((W,H),Image.LANCZOS)
base=np.asarray(ld('L_base.png','RGB'),float)/255
obj=np.asarray(ld('L_obj.png','L'),float)/255
bed=np.asarray(ld('L_bed.png','L'),float)/255
can=ld('L_canopy.png','RGBA'); fern=ld('L_fern.png','RGBA')
yy,xx=np.mgrid[0:H,0:W].astype(float)
def col(h): h=h.lstrip('#'); return np.array([int(h[i:i+2],16) for i in (0,2,4)],float)/255
# gravel field (same formulas, scaled)
GX,GY=1000*S,560*S; RX,RY=GX+20*S,GY+10*S; period=17.0*S
d=np.sqrt(((xx-RX)/1.28)**2+(yy-RY)**2); d2=np.sqrt(((xx-1380*S)/1.2)**2+(yy-720*S)**2)
Lx,Ly=-0.55,-0.83
dr_=np.maximum(d,1e-3); gxr=(xx-RX)/1.28/dr_; gyr=(yy-RY)/dr_
dr2=np.maximum(d2,1e-3); gx2=(xx-1380*S)/1.2/dr2; gy2=(yy-720*S)/dr2
sl=np.hypot(0.15,1.0)
zone1=(d<430*S); zone2=(d2<150*S)&~zone1
phase=np.where(zone1,d/period,np.where(zone2,d2/period,(yy+0.15*xx)/period))
gdx=np.where(zone1,gxr,np.where(zone2,gx2,0.15/sl)); gdy=np.where(zone1,gyr,np.where(zone2,gy2,1/sl))
speck=0.9+0.16*np.random.default_rng(12).random((H,W))
e=np.sqrt(((xx-GX)/(600*S))**2+((yy-GY)/(400*S))**2); conc=(1-0.16*np.clip(e-0.4,0,1))
def sand_at(ph,sl_,ys,xs):
    p=np.mod(ph,1.0); slope=-np.sin(p*2*math.pi)
    sh=np.clip(0.55+0.42*slope*(gdx[ys,xs]*(-Lx)+gdy[ys,xs]*(-Ly)),0,1)
    c=col('#7E8573')[None,None]*(1-sh[...,None])+col('#D8D9C6')[None,None]*sh[...,None]
    return c*(speck[ys,xs]*conc[ys,xs])[...,None]
# cursor path (figure-eight across the gravel)
def cursor(t):
    a=t*0.55; return (GX+280*S*math.sin(a), GY+120*S*math.sin(2*a))
trail=[]
# basin
BX,BY=245*S,600*S
ex,ey=(BX-18*S),(BY-14*S)
RAIN='rain' in sys.argv
# leaves
rng=random.Random(3)
LEAVES=[dict(x=rng.uniform(40,500)*S,y=rng.uniform(40,200)*S,t0=rng.uniform(0,DUR),sp=rng.uniform(18,30)*S,rot=rng.uniform(0,6),c=rng.choice([(110,40,34),(90,32,28),(128,56,40)])) for _ in range(9)]
FLIES=[(rng.uniform(0,W),rng.uniform(0,H),rng.uniform(0,6)) for _ in range(40)]
def tod(t):
    # 0..6 morning, 6..11 golden, 11..18 night
    k=[(0,np.array([0.97,1.04,1.02]),0.98),(3.5,np.array([0.97,1.04,1.02]),0.98),(5.5,np.array([1.02,1.02,0.97]),1.08),(8.0,np.array([1.02,1.02,0.97]),1.08),(10.0,np.array([1.22,0.94,0.68]),1.0),(12.5,np.array([1.22,0.94,0.68]),0.95),(14.5,np.array([0.6,0.72,1.0]),0.5),(18,np.array([0.6,0.72,1.0]),0.5)]
    for i in range(len(k)-1):
        if k[i][0]<=t<=k[i+1][0]:
            u=(t-k[i][0])/(k[i+1][0]-k[i][0]); u=u*u*(3-2*u)
            return k[i][1]*(1-u)+k[i+1][1]*u, k[i][2]*(1-u)+k[i+1][2]*u
    return k[-1][1],k[-1][2]
def fbm(oct,base_,seed):
    g=np.random.default_rng(seed); out=np.zeros((H,W)); amp=1; tot=0
    for o in range(oct):
        sz=base_*2**o; n=g.random((int(sz*H/W)+3,sz+3)); im=Image.fromarray((n*255).astype('uint8')).resize((W,H),Image.BICUBIC)
        out+=np.asarray(im,float)/255*amp; tot+=amp; amp*=.5
    return out/tot
dap=np.clip((fbm(4,9,77)-0.5)*4,0,1); lightf=np.clip(1-((xx-760*S)/(1300*S))**2-((yy-430*S)/(820*S))**2,0,1)
cano=np.clip(1-np.sqrt(((xx-W)/(950*S))**2+(yy/(650*S))**2),0,1)+np.clip(1-np.sqrt((xx/(700*S))**2+(yy/(520*S))**2),0,1)
STATIC=(0.93+0.12*dap*lightf)*(1-0.16*np.clip(cano,0,1)*np.clip((fbm(3,14,88)-0.35)*2,0,1))
vig=np.clip(1-0.32*(((xx-W/2)/(W*0.6))**2+((yy-H/2)/(H*0.62))**2),0.35,1)
def sway_layer(lay,ang_deg,pivot):
    return lay.rotate(ang_deg,resample=Image.BICUBIC,center=pivot)
def frame(t):
    img=base.copy()
    # ---- gravel ripple following cursor (wake that decays)
    cx,cy=cursor(t); trail.append((t,cx,cy))
    while trail and t-trail[0][0]>1.6: trail.pop(0)
    x0=int(max(0,min(cx,min(p[1] for p in trail))-140*S)); x1=int(min(W,max(cx,max(p[1] for p in trail))+140*S))
    y0=int(max(0,min(cy,min(p[2] for p in trail))-140*S)); y1=int(min(H,max(cy,max(p[2] for p in trail))+140*S))
    ys,xs=slice(y0,y1),slice(x0,x1)
    off=np.zeros((y1-y0,x1-x0))
    for (tt,px,py) in trail[::3]:
        age=t-tt; rr=np.sqrt((xx[ys,xs]-px)**2+(yy[ys,xs]-py)**2)
        ring=np.exp(-((rr-age*160*S)/(14*S))**2)*np.exp(-age*2.0)
        off+=0.35*ring*np.sin(rr/(9*S)-age*10)
    newsand=sand_at(phase[ys,xs]+off,None,ys,xs)
    m=(bed[ys,xs]*(1-obj[ys,xs]))[...,None]*np.clip(np.abs(off)*6,0,1)[...,None]
    img[ys,xs]=img[ys,xs]*(1-m)+newsand*m
    # ---- water stream shimmer & basin ripples
    pil=Image.fromarray((np.clip(img,0,1)*255).astype('uint8')); dr=ImageDraw.Draw(pil,'RGBA')
    spx,spy=ex+6*S,ey+5*S
    for k in range(3):
        u=((t*1.3+k/3)%1); rr=3*S+u*34*S; a=int(120*(1-u))
        dr.ellipse([spx-rr,spy-rr*0.95,spx+rr,spy+rr*0.95],outline=(200,232,226,a),width=max(1,int(2*S)))
    jit=1.2*S*math.sin(t*23)
    dr.ellipse([spx-3*S+jit,spy-3*S,spx+3*S+jit,spy+3*S],fill=(225,245,240,200))
    if RAIN:
        rg=random.Random(int(t*FPS))
        for k in range(26):  # raindrop rings on the basin water, stones and gravel
            x=rg.uniform(0,W); y=rg.uniform(0,H); rr=rg.uniform(3,9)*S
            dr.ellipse([x-rr,y-rr*0.9,x+rr,y+rr*0.9],outline=(220,232,236,70),width=1)
        for k in range(4):
            x=BX+rg.uniform(-30,30)*S; y=BY+rg.uniform(-28,28)*S; rr=rg.uniform(4,12)*S
            dr.ellipse([x-rr,y-rr,x+rr,y+rr],outline=(210,236,232,110),width=1)
    # ---- falling maple leaves
    for L in LEAVES:
        age=(t-L['t0'])%DUR
        if age<9:
            x=L['x']+age*L['sp']*1.2+math.sin(age*1.7+L['rot'])*30*S; y=L['y']+age*L['sp']*2.2
            r=10*S; a0=L['rot']+age*1.5; sq=0.55+0.45*abs(math.cos(age*2+L['rot']))
            pts=[(x+math.cos(a0+k/10*6.283)*r*(1 if k%2==0 else .45),y+math.sin(a0+k/10*6.283)*r*(1 if k%2==0 else .45)*sq) for k in range(10)]
            dr.polygon([(px+8*S,py+12*S) for px,py in pts],fill=(0,0,0,60)); dr.polygon(pts,fill=L['c']+(255,))
    img=np.asarray(pil,float)/255
    # ---- swaying ferns & canopy (with shadows)
    for lay,amp,piv,sh in [(fern,0.9,(1800*S,1080*S),(14*S,20*S,8*S,0.5)),(can,0.45,(W/2,-200*S),(34*S,48*S,26*S,0.6))]:
        sw=sway_layer(lay,amp*math.sin(t*0.9)+0.3*amp*math.sin(t*2.3),piv)
        a=np.asarray(sw,float)/255; mm=a[...,3]
        s_=Image.fromarray((mm*255).astype('uint8')).transform((W,H),Image.AFFINE,(1,0,-sh[0],0,1,-sh[1])).filter(ImageFilter.GaussianBlur(sh[2]))
        img*=1-sh[3]*(np.asarray(s_,float)/255)[...,None]
        lit=np.clip(1.1-0.3*(xx/W*0.5+yy/H*0.8),0.7,1.1)[...,None]
        img=img*(1-mm[...,None])+np.clip(a[...,:3]*lit,0,1)*mm[...,None]
    # ---- time of day grade
    tint,br=tod(t)
    if RAIN:
        g_=img.mean(axis=2,keepdims=True); img=img*0.62+g_*0.38          # desaturate
        img=np.clip((img-0.5)*1.08+0.5,0,1)*0.86                         # wet contrast, overcast
        tint,br=np.array([0.9,0.97,1.04]),0.88
    img=img*tint*br
    img*=(vig*STATIC)[...,None]
    # fireflies at night
    night=0 if RAIN else np.clip((t-13.6)/1.2,0,1)
    if night>0:
        pil=Image.fromarray((np.clip(img,0,1)*255).astype('uint8')); dr=ImageDraw.Draw(pil,'RGBA')
        for (fx,fy,ph) in FLIES:
            a=int(night*255*max(0,math.sin(t*2.2+ph))**2); x=fx+12*S*math.sin(t*0.6+ph); y=fy+10*S*math.cos(t*0.5+ph)
            dr.ellipse([x-5*S,y-5*S,x+5*S,y+5*S],fill=(255,240,150,a//4)); dr.ellipse([x-2*S,y-2*S,x+2*S,y+2*S],fill=(255,245,170,a))
        img=np.asarray(pil,float)/255
    if RAIN:
        pil=Image.fromarray((np.clip(img,0,1)*255).astype('uint8')); dr=ImageDraw.Draw(pil,'RGBA')
        rg=random.Random(int(t*FPS)*7)
        for k in range(420):
            x=rg.uniform(-100,W); y=rg.uniform(-60,H); l=rg.uniform(14,30)*S
            dr.line([(x,y),(x+l*0.25,y+l)],fill=(225,232,240,rg.randint(40,90)),width=1)
        img=np.asarray(pil,float)/255
    # cursor icon (preview only)
    pil=Image.fromarray((np.clip(img,0,1)*255).astype('uint8')); dr=ImageDraw.Draw(pil)
    c=[(cx,cy),(cx,cy+22*S),(cx+6*S,cy+17*S),(cx+10*S,cy+26*S),(cx+14*S,cy+24*S),(cx+10*S,cy+15*S),(cx+17*S,cy+15*S)]
    dr.polygon(c,fill=(255,255,255),outline=(0,0,0))
    return pil
if sys.argv[1]=='preview':
    if RAIN: tod=lambda t:(np.array([1,1,1]),1.0)
    ims=[frame(tt).resize((480,270)) for tt in [1.5,6.5,11.2,16.5]]
    G=Image.new('RGB',(960,540))
    for i,im in enumerate(ims): G.paste(im,((i%2)*480,(i//2)*270))
    G.save('anim_prev.png')
else:
    if RAIN: tod=lambda t:(np.array([1,1,1]),1.0)
    p=subprocess.Popen(['ffmpeg','-y','-loglevel','error','-f','rawvideo','-pix_fmt','rgb24','-s',f'{W}x{H}','-r',str(FPS),'-i','-','-c:v','libx264','-preset','medium','-crf','19','-pix_fmt','yuv420p','-movflags','+faststart',sys.argv[2]],stdin=subprocess.PIPE)
    n=int(DUR*FPS)
    for i in range(int(sys.argv[3]),int(sys.argv[4])):
        p.stdin.write(frame(i/FPS).tobytes())
    p.stdin.close(); p.wait(); print('done')
