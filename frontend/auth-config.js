(function () {
    const isLocalDev = location.hostname === 'localhost' || location.hostname === '127.0.0.1';
    const apiOrigin = isLocalDev ? 'http://localhost:3000' : window.location.origin;

    window.API_CONFIG = { baseUrl: apiOrigin };

    window.CURRENT_USER_EMAIL = localStorage.getItem('logged_in_email') || '';

    function setSessionCookie(email) {
        const maxAge = 144 * 60 * 60;
        document.cookie = `logged_in_email=${encodeURIComponent(email)}; path=/; max-age=${maxAge}; SameSite=Lax`;
    }

    const AUTH_CONFIG = {
        clientId: '135530387130-1v6j13pgrl79r0t1fg9mrsu6kd20rine.apps.googleusercontent.com',

        onSuccess: async (user) => {
            try {
                const response = await fetch(`${apiOrigin}/auth/verify?email=${encodeURIComponent(user.email)}`);
                if (!response.ok) {
                    throw new Error(`服务器响应异常: ${response.status}`);
                }

                const data = await response.json();

                if (!data || data.allowed !== true) {
                    alert('【安全拦截】您的账号不在白名单中，无权使用本系统！');
                    localStorage.removeItem('logged_in_email');
                    window.CURRENT_USER_EMAIL = '';
                    document.cookie = 'logged_in_email=; path=/; max-age=0';
                    if (window.google?.accounts?.id) {
                        window.google.accounts.id.disableAutoSelect();
                    }
                    location.reload();
                    return;
                }

                localStorage.setItem('logged_in_email', user.email);
                window.CURRENT_USER_EMAIL = user.email;
                setSessionCookie(user.email);

                console.log('白名单验证通过！');
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
})();
