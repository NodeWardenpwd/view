const AUTH_CONFIG = {
    // 替换为你在 Google 后台申请到的专属客户端 ID
    clientId: '135530387130-1v6j13pgrl79r0t1fg9mrsu6kd20rine.apps.googleusercontent.com',
    
    onSuccess: async (user) => {
        console.log('Authentication successful:', user.email);
        
        try {
            // 请求我们刚刚在 Rust 后端写好的验证接口
            const response = await fetch(`/api/auth/verify?email=${encodeURIComponent(user.email)}`);
            const data = await response.json();
            
            // 如果后端返回 allowed 为 false，说明邮箱对不上环境变量
            if (!data.allowed) {
                alert('未授权的账号！您无权查看此系统。');
                if (window.google?.accounts?.id) {
                    window.google.accounts.id.disableAutoSelect();
                }
                location.reload(); 
                return;
            }
            
            console.log("白名单验证通过，欢迎进入系统！");
            
            // 触发原项目自带的成功进入系统的控制（从 auth.js 里的实际行为来看，一般是隐藏遮罩层）
            if (typeof window.onAuthSuccess === 'function') {
                window.onAuthSuccess(user);
            }
            
        } catch (err) {
            console.error("安全验证服务器连接失败:", err);
            alert("认证系统故障，请检查后端服务");
        }
    },
    onError: (error) => {
        console.error('Authentication error:', error);
    }
};

window.AUTH_CONFIG = AUTH_CONFIG;