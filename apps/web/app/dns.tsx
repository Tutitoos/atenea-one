import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, result } from '@atenea/api-client';
import { dnsSchema } from './domain';
import { ActionForm, RunButton, Status } from './ui';
export function Dns({ net, canEdit }: { net: string; canEdit: boolean }) {
  const cache = useQueryClient();
  const query = useQuery({ queryKey: ['network', net, 'dns'], queryFn: () => result(api.GET('/api/v1/networks/{net}/dns', { params: { path: { net } } })) });
  return <section aria-label="DNS dividido"><h2>DNS dividido</h2><p>Solo enruta las consultas de los dominios indicados. Nunca captura el DNS general ni configura el dominio raíz «.». El agente necesita systemd-resolved; si no está disponible, informa unsupported y no modifica resolv.conf.</p><p>Hasta 3 servidores IP y 8 dominios LDH en minúsculas, sin punto final. Los servidores deben pertenecer a la red del túnel o a una ruta aprobada: el servidor valida su alcance. Vaciar ambos campos elimina la configuración.</p><Status query={query}/>{query.isError && <RunButton run={() => query.refetch()}>Reintentar DNS</RunButton>}{query.data && (canEdit ? <ActionForm label="Guardar DNS" fields={[{ name: 'servers', label: 'Servidores DNS (separados por espacios)', value: query.data.servers.join(' '), required: false }, { name: 'domains', label: 'Dominios de enrutamiento (separados por espacios)', value: query.data.domains.join(' '), required: false }]} submit={async values => {
    const body = dnsSchema.parse({ servers: values.servers.trim() ? values.servers.trim().split(/\s+/) : [], domains: values.domains.trim() ? values.domains.trim().split(/\s+/) : [] });
    await api.PUT('/api/v1/networks/{net}/dns', { params: { path: { net } }, body });
    await cache.invalidateQueries({ queryKey: ['network', net] });
  }}/> : <><p>Solo lectura</p><dl><dt>Servidores DNS</dt><dd>{query.data.servers.join(', ') || 'Sin servidores'}</dd><dt>Dominios de enrutamiento</dt><dd>{query.data.domains.join(', ') || 'Sin dominios'}</dd></dl></>)}</section>;
}
