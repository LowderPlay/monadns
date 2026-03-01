class AuthState {
  key = $state(localStorage.getItem('monadns_api_key') || '');
  needsLogin = $state(false);

  setKey(newKey: string) {
    this.key = newKey;
    this.needsLogin = false;
    localStorage.setItem('monadns_api_key', newKey);
  }

  logout() {
    this.key = '';
    this.needsLogin = true;
    localStorage.removeItem('monadns_api_key');
  }
}

export const auth = new AuthState();
