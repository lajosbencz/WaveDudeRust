import math, sys
def cround(x, d=0):
    # .NET Math.Round, half to even
    if d == 0:
        return float(round(x))
    return round(x, d)
SUP=4
def substeps(maxc2):
    # (double)f32 * f32(32/3) / 2f
    import struct
    f = struct.unpack('f', struct.pack('f', 32.0/3.0))[0]
    return max(1, int(round(math.ceil(math.sqrt(maxc2 * f / 2.0)))))
def Omega(lam):
    S = substeps(1.0)
    s = math.sin(2*math.pi/lam/(2*S))
    return 4*S*S*s*s
def Rows(first, count, prof):
    r=[]
    for j in range(count):
        s=0.0
        for k in range(SUP):
            s+=prof(first+j+(k+0.5)/SUP)
        r.append(cround(s/SUP,2))
    return r
def Mu(b): return math.cos(2*b)/6 - 8*math.cos(b)/3 + 2.5
def BetaOf(mu):
    lo,hi=0.0,math.pi
    for _ in range(100):
        m=(lo+hi)/2
        if Mu(m)<mu: lo=m
        else: hi=m
    return (lo+hi)/2
ITER=[0]
def Beta(index, omega, odd):
    n=len(index); pot=[omega*v*v for v in index]; top=max(0.0,max(pot))
    sigma=top+1e-9
    l0=[0.0]*n;l1=[0.0]*n;l2=[0.0]*n
    near=-4/3; far=1/12
    for j in range(n):
        if j>=2: l2[j]=far/l0[j-2]
        if j>=1:
            s=near-(l2[j]*l1[j-1] if j>=2 else 0.0); l1[j]=s/l0[j-1]
        d=sigma+2.5-pot[j]-l1[j]*l1[j]-l2[j]*l2[j]; l0[j]=math.sqrt(d)
    x=[]
    for j in range(n):
        t=(j+0.5)/n-0.5; x.append(t if odd else 1-t*t)
    mu=float('nan'); it=0
    for it in range(400):
        for j in range(n//2):
            a=x[j]; b=x[n-1-j]
            x[j]=(a-b)/2 if odd else (a+b)/2
            x[n-1-j]=-x[j] if odd else x[j]
        if n%2==1 and odd: x[n//2]=0.0
        s=math.sqrt(sum(v*v for v in x)); x=[v/s for v in x]
        at=lambda k: x[k] if 0<=k<n else 0.0
        nx=0.0
        for j in range(n):
            ax=-(at(j-2)+at(j+2))/12+4*(at(j-1)+at(j+1))/3-2.5*x[j]+pot[j]*x[j]
            nx+=x[j]*ax
        if abs(nx-mu)<1e-15:
            mu=nx; break
        mu=nx
        for j in range(n):
            s=x[j]
            if j>=1: s-=l1[j]*x[j-1]
            if j>=2: s-=l2[j]*x[j-2]
            x[j]=s/l0[j]
        for j in range(n-1,-1,-1):
            s=x[j]
            if j+1<n: s-=l1[j+1]*x[j+1]
            if j+2<n: s-=l2[j+2]*x[j+2]
            x[j]=s/l0[j]
    ITER[0]=max(ITER[0],it)
    edge=max(pot[0],pot[-1])
    if not (mu>edge): return None
    return BetaOf(mu)
def SingleBeta(lam,om,nc,ncl,w,clad):
    reach=clad if clad>0 else 2.0*lam
    half=math.ceil(w/2+reach+8)
    rows=Rows(-half,2*half,lambda y: nc if abs(y)<=w/2 else (ncl if abs(y)<=w/2+reach else 1.0))
    return Beta(rows,om,False)
def KappaAt(dist,om,nc,ncl,w,clad):
    core=w/2; outer=core+clad
    half=math.ceil(dist/2+outer+8)
    def p(y):
        ne=min(abs(y-dist/2),abs(y+dist/2))
        return nc if ne<=core else (ncl if ne<=outer else 1.0)
    rows=Rows(-half,2*half,p)
    e=Beta(rows,om,False); o=Beta(rows,om,True)
    return max(0.0,(e-o)/2) if (e is not None and o is not None) else 0.0
def BendShare(t): return t-math.sin(2*math.pi*t)/(2*math.pi)
def BendKappa(coupled,spacing,L,om,nc,ncl,w,outer):
    s=0.0
    for i in range(25):
        t=i/24; d=spacing-(spacing-coupled)*BendShare(t)
        wt=1.0 if i in (0,24) else (4.0 if i%2==1 else 2.0)
        s+=wt*KappaAt(d,om,nc,ncl,w,outer)
    return s*L/(3*24)
def design(lam,nc,ncl,port,shift):
    om=Omega(lam)
    ap=math.sqrt(nc*nc-ncl*ncl)
    w=max(2,int(math.floor(0.9*lam/(2*ap))))
    beta=SingleBeta(lam,om,nc,ncl,w,0)
    mu=Mu(beta); gamma=math.sqrt(max(1e-9,mu-om*ncl*ncl))
    clad=max(2,int(math.ceil(3/gamma))); absb=max(2,int(math.ceil(0.5*lam)))
    outer=clad+absb
    lowered=nc+0.1>2.55+1e-9; blead=shift>=0; onA=(blead==lowered)
    pidx=cround(nc-0.1 if lowered else nc+0.1,2)
    sb=SingleBeta(lam,om,nc,ncl,w,outer) or beta
    pb=(SingleBeta(lam,om,pidx,ncl,w,outer) or sb) if pidx>ncl else sb
    step=abs(pb-sb)
    region=max(1,int(cround(math.pi/2/step)))
    plen=min(region,int(cround(abs(shift)*math.pi/180/step)))
    radius=11.0*1.5*sb*sb/gamma**3
    gap=max(2,int(cround(1/gamma))); gap+=(w+gap)%2
    loops=0
    while True:
        loops+=1
        coupled=w+gap
        spacing=max(port,max(coupled+w+2*clad+2,int(math.ceil(w+2.0*(clad+absb)))))
        spacing+=spacing%2
        L=max(1,int(math.ceil(math.sqrt(2*math.pi*radius*(spacing-coupled)/2))))
        kappa=KappaAt(coupled,om,nc,ncl,w,outer)
        bend=BendKappa(coupled,spacing,L,om,nc,ncl,w,outer)
        print(f"  loop {loops}: gap {gap} coupled {coupled} spacing {spacing} bendL {L} kappa {kappa:.6f} bend {bend:.5f} 2bend {2*bend:.5f} lim {0.6*math.pi/4:.5f}")
        if 2*bend<=0.6*math.pi/4 or gap>=2*clad: break
        gap+=2
    cl=max(0,int(cround((math.pi/4-0.07-2*bend)/kappa))) if kappa>0 else 0
    d=dict(lam=lam,omega=om,aperture=ap,width=w,beta0=beta,mu=mu,gamma=gamma,clad=clad,absorber_min=absb,outer=outer,
           lowered=lowered,phaseOnA=onA,phaseIndex=pidx,singleBeta=sb,phaseBeta=pb,phaseStep=step,region=region,phaseLen=plen,
           radius=radius,gap=gap,coupled=coupled,spacing=spacing,bendL=L,kappa=kappa,bend2=2*bend,coupler=cl,
           inputL=int(cround(lam/2)),lead=int(cround(lam)),outL=int(cround(3*lam)),rampL=int(cround(2*lam)))
    d['length']=d['inputL']+d['lead']+region+d['lead']+2*L+cl+d['outL']
    d['absorberWidth']=spacing/2-w/2-clad
    d['left']=-d['length']/2; d['inputX']=d['left']+d['inputL']
    d['outputX']=d['length']/2-d['outL']+cround(lam/2)
    d['rampStart']=d['length']/2-d['rampL']
    d['phaseStart']=d['inputX']+d['lead']; d['phaseEnd']=d['phaseStart']+plen
    d['bendIn']=d['inputX']+2*d['lead']+region; d['couplerStart']=d['bendIn']+L; d['bendOut']=d['couplerStart']+cl; d['bendOutEnd']=d['bendOut']+L
    d['qwave']=lam/(4*math.sqrt(nc))
    return d
if __name__=='__main__':
    lam=cround(2*math.pi/(50/255*0.5*2.0),3)
    lamraw=2*math.pi/(50/255*0.5*2.0)
    print('lambda raw',repr(lamraw),'rounded',lam)
    for shift,name in ((90,'XOR'),(-90,'AND'),(30,'OR')):
        print(name)
        d=design(lam,2.55,1.45,2,shift)
        for k,v in d.items(): print(f"  {k} = {v!r}")
    print('max iterations used', ITER[0])
