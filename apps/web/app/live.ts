import { useEffect, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { Model } from '@atenea/api-client';
export function reconnectDelay(attempt: number, random = Math.random()) { return Math.min(30_000, 1000 * 2 ** Math.min(attempt, 5)) * (0.75 + random * 0.5); }
export function useNetworkEvents(net: string) {
  const client = useQueryClient();
  const [connected, setConnected] = useState(false);
  useEffect(() => {
    if (!net || typeof WebSocket === 'undefined') return;
    let stopped = false, attempt = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let socket: WebSocket;
    const invalidate = () => { void client.invalidateQueries({ queryKey: ['network', net] }); };
    function connect() {
      if (stopped) return;
      try { socket = new WebSocket(`${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/api/v1/networks/${encodeURIComponent(net)}/events`); }
      catch { timer = setTimeout(connect, reconnectDelay(attempt++)); return; }
      socket.onopen = () => { attempt = 0; setConnected(true); invalidate(); };
      socket.onmessage = event => {
        try { const value = JSON.parse(String(event.data)) as Model<'NetworkEvent'>; if (value.type === 'changed' || value.type === 'hello') invalidate(); } catch { /* Una trama desconocida no modifica el estado REST. */ }
      };
      socket.onerror = () => socket.close();
      socket.onclose = () => { if (!stopped) { setConnected(false); timer = setTimeout(connect, reconnectDelay(attempt++)); } };
    }
    connect();
    return () => { stopped = true; clearTimeout(timer); socket?.close(); setConnected(false); };
  }, [net, client]);
  return connected;
}
export function useClock() { const [now, setNow] = useState(Date.now); useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(timer); }, []); return now; }
