// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Link } from 'react-router';

// organization y network incluyen nombre e id; section es la etiqueta del registro.
export function Breadcrumbs({ organization, network, section }: {
  organization: { id: string; name: string };
  network?: { id: string; name: string };
  section?: string;
}) {
  return <nav className="breadcrumbs" aria-label="Migas de pan"><ol>
    <li><Link to={`/?org=${organization.id}`}>{organization.name}</Link></li>
    {network && <li><span aria-hidden="true">›</span><Link to={`/?org=${organization.id}&net=${network.id}&view=Resumen`}>{network.name}</Link></li>}
    {section && <li><span aria-hidden="true">›</span><span aria-current="page">{section}</span></li>}
  </ol></nav>;
}
