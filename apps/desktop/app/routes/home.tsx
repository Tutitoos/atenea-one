// Copyright 2026 Tutitoos
// SPDX-License-Identifier: Apache-2.0
import { Navigate } from 'react-router';

export default function Home() {
  return <Navigate to="/devices/self" replace />;
}
