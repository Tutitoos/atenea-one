// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { ApiError } from '@atenea/api-client';
import { ActionForm, Dialog } from './ui';
import { nameSchema } from './domain';

export function RenameDialog({ kind, name, rename }: { kind: 'organización' | 'red'; name: string; rename: (name: string) => Promise<unknown> }) {
  return <Dialog trigger={`Renombrar ${kind}`} title={`Renombrar ${kind}`} description="El identificador y los permisos no cambian.">
    {kind === 'red' && <p>Los códigos de unión ya emitidos conservan el nombre antiguo (solo informativo) y siguen siendo válidos.</p>}
    <ActionForm label="Guardar nombre" fields={[{ name: 'name', label: 'Nuevo nombre', value: name }]} submit={async values => {
      const parsed = nameSchema.safeParse(values.name);
      if (!parsed.success) throw new Error(parsed.error.issues[0].message);
      try { await rename(parsed.data); }
      catch (error) {
        if (error instanceof ApiError && error.status === 409) {
          const messages: Record<string, string> = {
            conflict: 'Ya existe una red con ese nombre en la organización.',
            invalid_network_name_for_code: 'El nombre no cabe en el código de unión. Prueba un nombre más corto.',
            invalid_public_url_for_code: 'El nombre no cabe en el código de unión con la URL pública actual. Prueba un nombre más corto.',
          };
          if (messages[error.detail.error.code]) throw new Error(messages[error.detail.error.code], { cause: error });
        }
        throw error;
      }
    }}/>
  </Dialog>;
}
