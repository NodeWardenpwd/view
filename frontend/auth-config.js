window.API_CONFIG = { baseUrl: '/' };

// 全局存储当前登录成功且通过验证的邮箱
window.CURRENT_USER_EMAIL = localStorage.getItem('logged_in_email') || '';

const AUTH_CONFIG = {
    clientId: '135530387130-1v6j13pgrl79r0t1fg9mrsu6kd20rine.apps.googleusercontent.com', // 填你自己的
    
    onSuccess: async (user) => {
        try {
            const response = await fetch(`/api/auth/verify?email=${encodeURIComponent(user.email)}`);
            if (!response.ok) {
                throw new Error(`服务器响应异常: ${response.status}`);
            }
            
            const data = await response.json();
            
            if (!data || data.allowed !== true) {
                alert('【安全拦截】您的账号不在白名单中，无权使用本系统！');
                localStorage.removeItem('logged_in_email');
                window.CURRENT_USER_EMAIL = '';
                if (window.google?.accounts?.id) {
                    window.google.accounts.id.disableAutoSelect();
                }
                location.reload(); 
                return;
            }
            
            // 验证通过，记录邮箱
            localStorage.setItem('logged_in_email', user.email);
            window.CURRENT_USER_EMAIL = user.email;
            
            console.log("白名单验证通过！");
            if (typeof window.onAuthSuccess === 'function') {
                window.onAuthSuccess(user);
            }
            
        } catch (err) {
            alert(`认证失败：${err.message}`);
            location.reload();
        }
    },
    onError: (error) => {
        console.error('Authentication error:', error);
    }
};

window.AUTH_CONFIG = AUTH_CONFIG;

// 【核心补丁】：拦截所有前端 fetch 请求，自动给后端送上 X-User-Email 供后端验身
const originalFetch = window.fetch;
window.fetch = function (url, options = {}) {
    options.headers = options.headers || {};
    if (window.CURRENT_USER_EMAIL) {
        options.headers['X-User-Email'] = window.CURRENT_USER_EMAIL;
    }
    return originalFetch(url, options);
};