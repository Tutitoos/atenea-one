import { act, renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, expect, it, vi } from 'vitest';
import { reconnectDelay, useNetworkEvents } from './live';
class Socket {
 static all:Socket[]=[];onopen:(()=>void)|null=null;onmessage:((e:{data:string})=>void)|null=null;onclose:(()=>void)|null=null;onerror:(()=>void)|null=null;
 constructor(){Socket.all.push(this);}close(){this.onclose?.();}
}
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals();Socket.all=[];});
it('reconecta con retroceso exponencial y jitter, invalida por REST y limpia timers',()=>{
 vi.useFakeTimers();vi.stubGlobal('WebSocket',Socket);vi.spyOn(Math,'random').mockReturnValue(0.5);
 const cache=new QueryClient();const invalidate=vi.spyOn(cache,'invalidateQueries');const wrapper=({children}:{children:React.ReactNode})=><QueryClientProvider client={cache}>{children}</QueryClientProvider>;
 const {result,unmount}=renderHook(()=>useNetworkEvents('red'),{wrapper});expect(result.current).toBe(false);
 act(()=>Socket.all[0].onopen?.());expect(result.current).toBe(true);expect(invalidate).toHaveBeenCalledWith({queryKey:['network','red']});
 act(()=>Socket.all[0].onmessage?.({data:'{"type":"changed","scope":"devices","seq":2}'}));expect(invalidate).toHaveBeenCalledTimes(2);
 for(const scope of ['routes','dns'])act(()=>Socket.all[0].onmessage?.({data:JSON.stringify({type:'changed',scope,seq:3})}));
 expect(invalidate).toHaveBeenCalledTimes(4);
 expect(invalidate).toHaveBeenLastCalledWith({queryKey:['network','red']});
 act(()=>Socket.all[0].close());expect(result.current).toBe(false);act(()=>vi.advanceTimersByTime(999));expect(Socket.all).toHaveLength(1);act(()=>vi.advanceTimersByTime(1));expect(Socket.all).toHaveLength(2);
 act(()=>Socket.all[1].close());act(()=>vi.advanceTimersByTime(1999));expect(Socket.all).toHaveLength(2);act(()=>vi.advanceTimersByTime(1));expect(Socket.all).toHaveLength(3);
 unmount();act(()=>vi.advanceTimersByTime(60_000));expect(Socket.all).toHaveLength(3);vi.restoreAllMocks();
});
it('acota el retraso y varía el jitter',()=>{expect(reconnectDelay(0,0)).toBe(750);expect(reconnectDelay(0,1)).toBe(1250);expect(reconnectDelay(50,0.5)).toBe(30_000);});
