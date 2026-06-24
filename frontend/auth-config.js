window.API_CONFIG = { baseUrl: '/' };

const AUTH_CONFIG = {
    clientId: '135530387130-1v6j13pgrl79r0t1fg9mrsu6kd20rine.apps.googleusercontent.com', // 记得填你自己的谷歌 ClientID
    
    onSuccess: async (user) => {
        console.log('Authentication successful:', user.email);
        
        try {
            // 请求我们写好的验证接口
            const response = await fetch(`/api/auth/verify?email=${encodeURIComponent(user.email)}`);
            
            // 【核心防御】：如果网络不通、Nginx 报错或者返回 404/500，一律拦截！
            if (!response.ok) {
                throw new Object({ message: `服务器响应异常: ${response.status}` });
            }
            
            const data = await response.json();
            
            // 如果后端返回 allowed 为 false，说明邮箱不在 Hugging Face 的环境变量里
            if (!data || data.allowed !== true) {
                alert('未授权的账号！您无权查看此系统。');
                if (window.google?.accounts?.id) {
                    window.google.accounts.id.disableAutoSelect();
                }
                location.reload(); 
                return;
            }
            
            console.log("白名单验证通过，欢迎进入系统！");
            if (typeof window.onAuthSuccess === 'function') {
                window.onAuthSuccess(user);
            }
            
        } catch (err) {
            console.error("安全验证失败:", err);
            alert(`认证失败：${err.message || '网络或接口异常'}`);
            if (window.google?.accounts?.id) {
                window.google.accounts.id.disableAutoSelect();
            }
            location.reload();
        }
    },
    onError: (error) => {
        console.error('Authentication error:', error);
    }
};

window.AUTH_CONFIG = AUTH_CONFIG;