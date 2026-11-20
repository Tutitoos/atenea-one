-- Copyright 2026 Tutitoos
-- SPDX-License-Identifier: Apache-2.0
-- ADR-031 (adenda): un binario anterior a la 0.1.36 aprueba con `UPDATE devices SET status='approved'` y no fija el orden
-- de aprobación ni el ajuste vigente. Al volver a subir, esa aprobación contaría como anterior a todo y se conectaría con los
-- aprobados después con el ajuste activo, aunque estuviera apagado al aprobarla. Esta función aplica en la base de datos la
-- misma regla que `issue::record_approval`: solo la primera transición a aprobado (orden 0), con el máximo de la red más
-- uno y el ajuste que tenga la red en ese momento. El bloqueo de la fila de la red lo toman ya todas las rutas de
-- aprobación antes de escribir; aquí se repite para que el orden no dependa de ello.
CREATE FUNCTION devices_approval_order() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM 1 FROM networks WHERE id = NEW.network_id FOR UPDATE;
  NEW.approval_order := (SELECT COALESCE(max(approval_order), 0) + 1 FROM devices WHERE network_id = NEW.network_id);
  NEW.auto_connect_on_approval := (SELECT auto_connect FROM networks WHERE id = NEW.network_id);
  RETURN NEW;
END
$$;
CREATE TRIGGER devices_approval_order
  BEFORE UPDATE OF status ON devices
  FOR EACH ROW
  WHEN (NEW.status = 'approved' AND OLD.status IS DISTINCT FROM 'approved' AND NEW.approval_order = 0)
  EXECUTE FUNCTION devices_approval_order();
