import sys; sys.path.insert(0, '/tmp/claude-1000/-home-lazos-projects-github-com-graealex-WaveDudeNext/520d10e1-d1f7-49c6-8e89-c4fb4b525931/scratchpad')
import math, gate as g
lam=32.044
om=g.Omega(lam)
S=g.substeps(1.0)
print('S',S,'omega_t',2*math.pi/lam,'Omega',om, 'omega^2 (S=inf limit)',(2*math.pi/lam)**2)
w=6; half=math.ceil(w/2+2*lam+8)
rows=g.Rows(-half,2*half,lambda y: 2.55 if abs(y)<=w/2 else (1.45 if abs(y)<=w/2+2*lam else 1.0))
print('half',half,'n',len(rows))
# print the transition rows
for j,v in enumerate(rows):
    y=-half+j
    if j==0 or (j>0 and v!=rows[j-1]) or j==len(rows)-1: print('  row',j,'y',y,'..',y+1,'n',v)
g.ITER[0]=0
b=g.Beta(rows,om,False); print('beta even',b,'iters',g.ITER[0])
mu=g.Mu(b); print('mu',mu,'edge pot',om*rows[0]**2,'top',om*2.55**2)
print('n_eff = beta/k0', b/(2*math.pi/lam))
gam=math.sqrt(mu-om*1.45**2); print('gamma',gam,'1/gamma',1/gam)
g.ITER[0]=0
bo=g.Beta(rows,om,True); print('odd',bo,'iters',g.ITER[0])
# outer=27 variant
half2=math.ceil(3+27+8)
rows2=g.Rows(-half2,2*half2,lambda y: 2.55 if abs(y)<=3 else (1.45 if abs(y)<=30 else 1.0))
print('outer27 n',len(rows2),'beta',g.Beta(rows2,om,False))
rows3=g.Rows(-half2,2*half2,lambda y: 2.45 if abs(y)<=3 else (1.45 if abs(y)<=30 else 1.0))
print('phase 2.45 beta',g.Beta(rows3,om,False))
# coupled pair distance 12
for d in (10,12,20,60):
    hh=math.ceil(d/2+30+8)
    print('kappa d',d, g.KappaAt(d,om,2.55,1.45,6,27),'n rows',2*hh)
# continuous slab TE check: k0^2 n^2 - beta^2 ... compare
# library_check fixture
lam2=16.0; om2=g.Omega(lam2)
r=g.Rows(-57,114,lambda y: 1.6 if abs(y)<=4 else (1.45 if abs(y)<=53 else 1.0))
print('fixture lambda 16 Omega',om2,'beta',g.Beta(r,om2,False))
# BetaOf range check
print('Mu(pi)',g.Mu(math.pi))
