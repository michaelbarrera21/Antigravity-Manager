@echo off
REM ECDSA Root CA 证书生成脚本 (Windows)
REM 仅支持 ECDSA P-256/P-384

setlocal enabledelayedexpansion

echo === ECDSA Root CA 证书生成脚本 ===
echo.

REM 默认参数
set CURVE=prime256v1
set DAYS=3650
set OUTPUT_DIR=.

REM 解析参数
:parse_args
if "%1"=="" goto :start
if "%1"=="--curve" (
    set CURVE=%2
    shift
    shift
    goto :parse_args
)
if "%1"=="--days" (
    set DAYS=%2
    shift
    shift
    goto :parse_args
)
if "%1"=="--output-dir" (
    set OUTPUT_DIR=%2
    shift
    shift
    goto :parse_args
)
if "%1"=="-h" goto :help
if "%1"=="--help" goto :help
echo 未知参数: %1
echo 使用 -h 或 --help 查看帮助
exit /b 1

:help
echo 用法: %0 [选项]
echo.
echo 选项:
echo   --curve CURVE    椭圆曲线 (prime256v1^|secp384r1)，默认: prime256v1
echo   --days DAYS       有效期天数，默认: 3650
echo   --output-dir DIR  输出目录，默认: 当前目录
echo   -h, --help       显示帮助信息
echo.
echo 示例:
echo   %0                           # 使用默认参数生成 P-256 证书
echo   %0 --curve secp384r1        # 生成 P-384 证书
echo   %0 --days 7300               # 生成 20 年有效期的证书
exit /b 0

:start
REM 验证曲线类型
if "%CURVE%"=="prime256v1" goto :curve_ok
if "%CURVE%"=="secp384r1" goto :curve_ok
echo 错误: 不支持的曲线类型: %CURVE%
echo 支持的曲线: prime256v1 (P-256), secp384r1 (P-384)
exit /b 1

:curve_ok
REM 创建输出目录
if not exist "%OUTPUT_DIR%" mkdir "%OUTPUT_DIR%"

REM 生成私钥
echo 1. 生成 ECDSA 私钥 (曲线: %CURVE%)...
openssl ecparam -genkey -name %CURVE% -noout -out "%OUTPUT_DIR%\ca-key.pem"
if errorlevel 1 (
    echo 私钥生成失败
    exit /b 1
)
echo    私钥已保存到: %OUTPUT_DIR%\ca-key.pem

REM 生成证书
echo 2. 生成 Root CA 证书...
openssl req -new -x509 -key "%OUTPUT_DIR%\ca-key.pem" -out "%OUTPUT_DIR%\ca-cert.pem" -days %DAYS% -subj "/CN=Antigravity-MITM-CA"
if errorlevel 1 (
    echo 证书生成失败
    exit /b 1
)
echo    证书已保存到: %OUTPUT_DIR%\ca-cert.pem

REM 验证文件
echo.
echo 3. 验证生成的文件...
if exist "%OUTPUT_DIR%\ca-key.pem" (
    if exist "%OUTPUT_DIR%\ca-cert.pem" (
        echo    ✓ 私钥文件存在
        echo    ✓ 证书文件存在
        
        echo.
        echo 4. 证书信息:
        openssl x509 -in "%OUTPUT_DIR%\ca-cert.pem" -text -noout | findstr /i "Subject: Not Before: Not After: Public Key Algorithm: Signature Algorithm:"
        
        echo.
        echo === 生成完成 ===
        echo 私钥: %OUTPUT_DIR%\ca-key.pem
        echo 证书: %OUTPUT_DIR%\ca-cert.pem
        echo.
        echo 下一步:
        echo 1. 将证书安装到系统受信任的根证书颁发机构
        echo 2. 在 Antigravity 中配置 MITM 代理
        echo 3. 使用代理地址: 127.0.0.1:8081
    ) else (
        echo    ✗ 证书文件不存在
        exit /b 1
    )
) else (
    echo    ✗ 私钥文件不存在
    exit /b 1
)
