// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/ComputedImageCache.hpp"
#include "support/RenderTestSuites.hpp"

#include <QSemaphore>
#include <QTest>
#include <QThreadPool>
#include <QtConcurrentRun>

#include <atomic>
#include <new>

namespace ugurugu
{

class ComputedImageCacheTests final : public QObject
{
    Q_OBJECT

private slots:
    void computesAConcurrentMissOnce()
    {
        render_detail::ComputedImageCache cache(qint64{1024} * 1024);
        QImage expected(QSize(4, 4), QImage::Format_ARGB32_Premultiplied);
        expected.fill(Qt::blue);
        std::atomic_int computations = 0;
        QSemaphore entered;
        QSemaphore release;
        QThreadPool pool;
        pool.setMaxThreadCount(2);
        const auto compute = [&]()
        {
            ++computations;
            entered.release();
            release.acquire();
            return expected;
        };
        QFuture<QImage> first = QtConcurrent::run(&pool,
            [&]()
            {
                return cache.imageOrCompute(QStringLiteral("key"), compute);
            });
        QVERIFY(entered.tryAcquire(1, 5000));
        QFuture<QImage> second = QtConcurrent::run(&pool,
            [&]()
            {
                return cache.imageOrCompute(QStringLiteral("key"), compute);
            });
        release.release(2);
        first.waitForFinished();
        second.waitForFinished();
        QCOMPARE(first.result(), expected);
        QCOMPARE(second.result(), expected);
        QCOMPARE(computations.load(), 1);
    }

    void releasesAKeyWhoseComputationThrew()
    {
        render_detail::ComputedImageCache cache(qint64{1024} * 1024);
        QImage expected(QSize(4, 4), QImage::Format_ARGB32_Premultiplied);
        expected.fill(Qt::red);
        QSemaphore entered;
        QSemaphore release;
        QThreadPool pool;
        pool.setMaxThreadCount(2);
        QFuture<bool> owner = QtConcurrent::run(&pool,
            [&]()
            {
                try
                {
                    cache.imageOrCompute(QStringLiteral("key"),
                        [&]() -> QImage
                        {
                            entered.release();
                            release.acquire();
                            throw std::bad_alloc();
                        });
                }
                catch (const std::bad_alloc &)
                {
                    return true;
                }
                return false;
            });
        QVERIFY(entered.tryAcquire(1, 5000));
        QFuture<QImage> waiter = QtConcurrent::run(&pool,
            [&]()
            {
                return cache.imageOrCompute(QStringLiteral("key"),
                    [&expected]()
                    {
                        return expected;
                    });
            });
        release.release();
        QTRY_VERIFY_WITH_TIMEOUT(owner.isFinished(), 5000);
        QVERIFY(owner.result());
        QTRY_VERIFY_WITH_TIMEOUT(waiter.isFinished(), 5000);
        QCOMPARE(waiter.result(), expected);
        QCOMPARE(cache.imageOrCompute(QStringLiteral("key"),
                     []()
                     {
                         return QImage();
                     }),
            expected);
    }
};

int runComputedImageCacheTests(int argc, char **argv)
{
    ComputedImageCacheTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "ComputedImageCacheTests.moc"
