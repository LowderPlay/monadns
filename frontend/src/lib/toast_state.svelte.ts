export type ToastType = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  message: string;
  type: ToastType;
}

class ToastState {
  toasts = $state<Toast[]>([]);
  private nextId = 0;

  show(message: string, type: ToastType = 'info', duration = 3000) {
    const id = this.nextId++;
    this.toasts.push({ id, message, type });
    
    setTimeout(() => {
      this.toasts = this.toasts.filter(t => t.id !== id);
    }, duration);
  }

  success(message: string) { this.show(message, 'success'); }
  error(message: string) { this.show(message, 'error'); }
  info(message: string) { this.show(message, 'info'); }
}

export const toast = new ToastState();
