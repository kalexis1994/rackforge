"""Native Hermite source spectra and independent capture-rate diagnostics.

Windows multiply physical acceleration, never tapered displacement. Pressure
uses the same frozen microphone kernel. Short-window comparisons attribute
numerical errors; they do not certify full-note or full-phrase spectra.
"""
import time
import numpy as np
from scipy.interpolate import CubicHermiteSpline
from concert_grand_uniform_spectra import uniform_window_spectra
from concert_grand_window_spectra import band_norms


BANDS = [(60.,800.),(800.,2500.),(2500.,6000.),(6000.,12000.)]


def frequency_grid(max_step=25.):
    if not np.isfinite(max_step) or max_step<=0: raise ValueError('positive frequency spacing')
    grid=[]
    for low,high in BANDS:
        # Even counts make the half-density grid a nested independent control.
        count=2*int(np.ceil((high-low)/(2*max_step)))
        part=np.linspace(low,high,count+1)
        grid.extend(part if not grid else part[1:])
    return np.asarray(grid)


def pressure_spectra(trace, frequency, transfer):
    source=uniform_window_spectra(*trace,frequency)
    result={label:np.einsum('fcm,fm->fc',transfer,pair[1]) for label,pair in source.items()}
    if not all(np.isfinite(x).all() for x in result.values()): raise ValueError('nonfinite source pressure')
    return result


def capture(trace, rate=128000):
    t,B,V=trace; duration=t[-1]-t[0]
    count=round(duration*rate)
    if abs(count/rate-duration)>1e-14: raise ValueError('aligned capture window required')
    native=round(1/(t[1]-t[0]))
    if native==rate: return trace
    if native%rate==0:
        stride=native//rate
        return t[::stride],B[::stride],V[::stride]
    uniform=t[0]+np.arange(count+1)/rate
    spline=CubicHermiteSpline(t,B,V,axis=0)
    return uniform,spline(uniform),spline(uniform,1)


def compare_spectra(frequency, actual, reference):
    base=band_norms(frequency,reference,BANDS)
    delta=band_norms(frequency,actual-reference,BANDS)
    total=sum(base.values()); difference=sum(delta.values())
    rows=[]
    for low,high in BANDS:
        label=f'{low:g}-{high:g}'; norm=base[label]
        if norm<=0: raise ValueError('nonzero reference band required')
        rows.append(dict(lower_hz=low,upper_hz=high,
                         relative=float(np.sqrt(delta[label]/norm)),
                         reference_energy_share=norm/total,
                         absolute_relative_to_total=float(np.sqrt(delta[label]/total))))
    return dict(total_relative=float(np.sqrt(difference/total)),bands=rows,
                all_bands_1_percent_passed=all(r['relative']<=.01 for r in rows))


def native_run(pilot,initial,duration,rate):
    state=tuple(x.copy() for x in initial); core=pilot.system.string
    count=round(duration*rate)
    if count/rate!=duration: raise ValueError('aligned native source interval')
    def board(q): return core.board_transform@q[core.sb]
    B=[board(state[0])]; V=[board(state[1])]
    felt=material=force=0.; start=last=time.perf_counter()
    for i in range(count):
        state,stats=pilot.step(state,1/rate,True)
        felt+=stats['felt_loss_j']; material+=stats['material_loss_j']; force=max(force,stats['force_n'])
        B.append(board(state[0])); V.append(board(state[1]))
        if time.perf_counter()-last>25:
            print('native source',rate,(i+1)/rate,'/',duration,flush=True); last=time.perf_counter()
    trace=(np.arange(count+1)/rate,np.asarray(B),np.asarray(V))
    if not all(np.isfinite(x).all() for x in (*state,*trace)): raise ValueError('nonfinite source/state')
    return state,trace,dict(rate_hz=rate,duration_s=duration,seconds=time.perf_counter()-start,
                           native_frames=count+1,felt_heat_j=felt,material_heat_j=material,
                           maximum_discrete_work_force_n=force,
                           final_compression_m=pilot.system.compression(state[0]))
