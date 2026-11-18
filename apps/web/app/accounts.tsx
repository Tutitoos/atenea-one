// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { useState } from 'react';
import { Button, CopyButton, Field } from './ui';

// Mensajes de los códigos del control plane (ADR-028). No se muestra nunca el texto crudo del servidor.
const messages: Record<string, string> = {
  invalid_credentials: 'Correo o contraseña incorrectos.',
  rate_limited: 'Demasiados intentos. Espera unos minutos y vuelve a probar.',
  registration_closed: 'El registro de cuentas está cerrado en este servidor.',
  invalid_login: 'Escribe un correo con formato válido.',
  conflict: 'Ya existe una cuenta con ese correo.',
  invalid_password: 'La contraseña es demasiado corta (el servidor pide 12 caracteres o más por defecto) o supera los 128 bytes.',
};

// Sin sesión no hay token CSRF: estas rutas no pasan por el cliente con sesión y exigen cuerpo JSON.
async function post(path: string, body: Record<string, string>): Promise<unknown> {
  const response = await fetch(path, {
    method: 'POST', credentials: 'same-origin', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body),
  });
  if (response.ok) return response.status === 204 ? undefined : response.json();
  const code = await response.json().then((json: { error?: { code?: string } }) => json.error?.code, () => undefined);
  throw new Error(messages[code ?? ''] ?? 'No se pudo completar la acción.');
}

type Mode = 'entrar' | 'registrar' | 'recuperar';
const titles: Record<Mode, string> = { entrar: 'Entrar con correo', registrar: 'Crear una cuenta', recuperar: 'Recuperar la contraseña' };

/** Códigos de recuperación de un solo uso: se muestran una vez y no se pueden volver a pedir. */
export function RecoveryCodes({ codes, done }: { codes: string[]; done: () => void }) {
  const [saved, setSaved] = useState(false);
  return <div className="card" role="region" aria-label="Códigos de recuperación">
    <h2>Guarda tus códigos de recuperación</h2>
    <p>Son la única forma de recuperar la cuenta si olvidas la contraseña: este servidor no envía correos. Cada código
      sirve una vez. <strong>No se volverán a mostrar.</strong></p>
    <ol>{codes.map(code => <li key={code}><code>{code}</code></li>)}</ol>
    <CopyButton value={codes.join('\n')} label="Copiar los códigos"/>
    <div className="incorporar-option"><label><input type="checkbox" checked={saved} onChange={event => setSaved(event.target.checked)}/> He guardado los códigos en un lugar seguro</label></div>
    <Button variant="primary" disabled={!saved} onClick={done}>Continuar</Button>
  </div>;
}

/** Correo y contraseña propios del servidor (ADR-028 D2); GitHub y el resto llegan por OIDC con el botón de la pantalla. */
export function LocalAccess({ onDone = () => window.location.assign('/') }: { onDone?: () => void }) {
  const [mode, setMode] = useState<Mode>('entrar');
  const [values, setValues] = useState({ login: '', password: '', name: '', code: '' });
  const [message, setMessage] = useState('');
  const [codes, setCodes] = useState<string[]>();
  const [pending, setPending] = useState(false);
  const set = (key: keyof typeof values) => (event: React.ChangeEvent<HTMLInputElement>) =>
    setValues({ ...values, [key]: event.target.value });
  const switchTo = (next: Mode) => { setMode(next); setMessage(''); };
  if (codes) return <RecoveryCodes codes={codes} done={onDone}/>;
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setMessage(''); setPending(true);
    try {
      if (mode === 'entrar') { await post('/auth/local/login', { login: values.login, password: values.password }); onDone(); }
      else if (mode === 'registrar') {
        const created = await post('/auth/local/register', {
          login: values.login, password: values.password, ...(values.name.trim() ? { display_name: values.name.trim() } : {}),
        }) as { recovery_codes: string[] };
        setCodes(created.recovery_codes);
      } else {
        await post('/auth/local/recover', { login: values.login, recovery_code: values.code, new_password: values.password });
        setValues({ ...values, password: '', code: '' }); setMode('entrar');
        setMessage('Contraseña cambiada. Se han cerrado las demás sesiones: entra con la nueva.');
      }
    } catch (error) { setMessage(error instanceof Error ? error.message : 'No se pudo completar la acción.'); }
    finally { setPending(false); }
  };
  return <form className="action-form" onSubmit={event => void submit(event)} aria-label={titles[mode]}>
    <h2>{titles[mode]}</h2>
    <Field label="Correo">{props => <input {...props} type="email" autoComplete="username" required value={values.login} onChange={set('login')}/>}</Field>
    {mode === 'registrar' && <Field label="Nombre visible (opcional)">{props =>
      <input {...props} type="text" maxLength={255} value={values.name} onChange={set('name')}/>}</Field>}
    {mode === 'recuperar' && <Field label="Código de recuperación">{props =>
      <input {...props} type="text" autoComplete="off" required value={values.code} onChange={set('code')}/>}</Field>}
    <Field label={mode === 'recuperar' ? 'Contraseña nueva' : 'Contraseña'}
      help={mode === 'entrar' ? undefined : 'Mejor una frase larga. El servidor rechaza las demasiado cortas.'}>{props =>
      <input {...props} type="password" required
        autoComplete={mode === 'entrar' ? 'current-password' : 'new-password'} value={values.password} onChange={set('password')}/>}</Field>
    <Button type="submit" variant="primary" loading={pending}>{{ entrar: 'Entrar', registrar: 'Crear cuenta', recuperar: 'Cambiar contraseña' }[mode]}</Button>
    <p role="status">{message}</p>
    <p className="muted">{mode !== 'entrar' && <Button type="button" variant="link" onClick={() => switchTo('entrar')}>Ya tengo cuenta</Button>}
      {mode !== 'registrar' && <Button type="button" variant="link" onClick={() => switchTo('registrar')}>Crear una cuenta</Button>}
      {mode !== 'recuperar' && <Button type="button" variant="link" onClick={() => switchTo('recuperar')}>He olvidado la contraseña</Button>}</p>
  </form>;
}
